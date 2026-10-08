//! HTTPS management routes. Tests call `router` without the TLS listener.

use std::collections::BTreeMap;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

use axum::body::Body;
use axum::extract::{DefaultBodyLimit, Path, State};
use axum::http::{HeaderMap, Request, StatusCode, Uri};
use axum::middleware::{self, Next};
use axum::response::Response;
use axum::routing::{get, post};
use axum::Router;
use serde_json::{json, Value};

use crate::canonical::canonical_sha256;
use crate::config::{binaries_ready, project_input_hash, AppConfig};
use crate::error::ApiError;
use crate::identity::{self, VerifiedIdentity};
use crate::ids::{build_id_ok, hex64, idempotency_ok, ident_ok, secret_ref_ok, session_cookie_ok};
use crate::jsonutil::parse_exact;
use crate::membership::{self, member_matches};
use crate::scope::{self, allows};
use crate::stages::preview_classification;
use crate::store::{self, BuildRow, CreateOutcome, StageRow};
use crate::CONTRACT;

pub struct AppState {
    pub config: AppConfig,
    pub cancels: Arc<Mutex<BTreeMap<String, Arc<AtomicBool>>>>,
}

pub fn router(config: AppConfig) -> Router {
    router_from(Arc::new(AppState {
        config,
        cancels: Arc::new(Mutex::new(BTreeMap::new())),
    }))
}

pub fn router_from(state: Arc<AppState>) -> Router {
    Router::new()
        .route("/control/v1/builds", post(create_build).get(list_builds))
        .route("/control/v1/builds/{build_id}", get(get_build))
        .route("/control/v1/builds/{build_id}/cancel", post(cancel_build))
        .route("/control/v1/builds/{build_id}/cards", get(get_cards))
        .route("/control/v1/builds/{build_id}/candidates", get(get_candidates))
        .route("/control/v1/builds/{build_id}/pack", post(compile_pack).get(get_pack))
        .route("/control/v1/reviews", get(list_reviews))
        .route("/control/v1/reviews/preview", post(preview_reviews))
        .route("/control/v1/reviews/{item_id}", get(get_review))
        .route("/control/v1/reviews/{item_id}/decision", post(decide_review))
        .route("/control/v1/classifications", get(list_classifications))
        .route("/control/v1/classifications/preview", post(preview_classes))
        .route("/control/v1/classifications/confirm", post(confirm_classes))
        .fallback(not_found)
        .method_not_allowed_fallback(method_not_allowed)
        .layer(DefaultBodyLimit::max(65_536))
        .layer(middleware::from_fn(normalize_framework_errors))
        .with_state(state)
}

async fn normalize_framework_errors(request: Request<Body>, next: Next) -> Response {
    let response = next.run(request).await;
    if response.status() == StatusCode::PAYLOAD_TOO_LARGE {
        return json_err(false, &ApiError::malformed());
    }
    response
}

async fn not_found() -> Response {
    json_err(false, &ApiError::not_found())
}

async fn method_not_allowed() -> Response {
    json_err(false, &ApiError::method())
}

async fn create_build(State(state): State<Arc<AppState>>, headers: HeaderMap, body: axum::body::Bytes) -> Response {
    let value = match parse_exact(&body) {
        Ok(value) => value,
        Err(error) => return json_err(false, &error),
    };
    if membership::reject_forged(&value).is_err() {
        return json_err(false, &ApiError::malformed());
    }
    if session_cookie(&headers).is_none() {
        return json_err(false, &ApiError::unauthenticated());
    }
    let request = match create_fields(&value) {
        Ok(request) => request,
        Err(error) => return json_err(false, &error),
    };
    let identity = match open_mutation(&state, &headers, request.revision).await {
        Ok(identity) => identity,
        Err(error) => return mutation_denied(error),
    };
    let Some(project_auth) = identity.project(&request.project_id) else {
        return json_err(true, &ApiError::forbidden());
    };
    if !allows(&project_auth.capabilities, scope::BUILD_CREATE) {
        return json_err(true, &ApiError::forbidden());
    }
    let Some(project) = state.config.projects.iter().find(|item| item.project_id == request.project_id) else {
        return json_err(true, &ApiError::forbidden());
    };
    if project.config_commit != request.config_commit
        || project.source_ref != request.source_ref
        || project.allowance_id != request.allowance_id
        || project.llm_secret_ref != request.llm_secret_ref
    {
        return json_err(true, &ApiError::forbidden());
    }
    if !binaries_ready(&state.config.binaries) {
        return json_err(true, &ApiError::not_ready());
    }
    let body_sha = canonical_sha256(&value);
    let build_id = match store::new_build_id().await {
        Ok(build_id) => build_id,
        Err(error) => return json_err(true, &error),
    };
    let job = store::NewBuild {
        build_id,
        project_id: request.project_id,
        idempotency_key: request.idempotency_key,
        body_sha256: body_sha,
        config_commit: request.config_commit,
        source_ref: request.source_ref,
        allowance_id: request.allowance_id,
        llm_secret_ref: request.llm_secret_ref,
        identity_revision: request.revision,
        subject: identity.subject.clone(),
        role_name: project_auth.role.clone(),
        input_sha256: project_input_hash(project),
    };
    match store::create_build(&state.config.postgres, &job).await {
        Ok(CreateOutcome::Created(row) | CreateOutcome::Replay(row)) => {
            let stages = store::stages(&state.config.postgres, &row.build_id).await.unwrap_or_default();
            json_ok(202, &build_document("createBuild", &row, &stages))
        }
        Ok(CreateOutcome::Conflict) => json_err(true, &ApiError::idempotency()),
        Ok(CreateOutcome::Unknown) => json_err(true, &ApiError::commit_unknown()),
        Err(error) => json_err(true, &error),
    }
}

async fn list_builds(State(state): State<Arc<AppState>>, headers: HeaderMap, uri: Uri) -> Response {
    let Some(project_id) = query_value(&uri, "project_id") else {
        return json_err(false, &ApiError::malformed());
    };
    let Some(identity) = gate_read(&state, &headers, &project_id, scope::BUILD_READ).await else {
        return gate_read_response(&state, &headers, &project_id, scope::BUILD_READ).await;
    };
    let _ = identity;
    match store::list_builds(&state.config.postgres, &project_id).await {
        Ok(rows) => {
            let mut builds = Vec::new();
            for row in rows {
                let stages = store::stages(&state.config.postgres, &row.build_id).await.unwrap_or_default();
                builds.push(build_document("listBuilds", &row, &stages));
            }
            json_ok(
                200,
                &json!({
                    "contract": CONTRACT,
                    "operation": "listBuilds",
                    "builds": builds,
                    "management_identity": "verified",
                    "published": false,
                    "active": false,
                    "data_grants": "none",
                    "compiled_pack": "not_published"
                }),
            )
        }
        Err(error) => json_err(true, &error),
    }
}

async fn get_build(State(state): State<Arc<AppState>>, headers: HeaderMap, Path(build_id): Path<String>) -> Response {
    read_build(&state, &headers, &build_id, scope::BUILD_READ, "getBuild").await
}

async fn cancel_build(State(state): State<Arc<AppState>>, headers: HeaderMap, Path(build_id): Path<String>, body: axum::body::Bytes) -> Response {
    let value = match parse_exact(&body) {
        Ok(value) => value,
        Err(error) => return json_err(false, &error),
    };
    let revision = match value.get("identity_revision").and_then(Value::as_i64) {
        Some(revision) if revision >= 1 => revision,
        _ => return json_err(false, &ApiError::malformed()),
    };
    let identity = match open_mutation(&state, &headers, revision).await {
        Ok(identity) => identity,
        Err(error) => return mutation_denied(error),
    };
    if !build_id_ok(&build_id) {
        return json_err(true, &ApiError::forbidden());
    }
    let Some(row) = load_scoped(&state, &identity, &build_id, scope::BUILD_CANCEL).await else {
        return json_err(true, &ApiError::forbidden());
    };
    if let Ok(map) = state.cancels.lock()
        && let Some(flag) = map.get(&row.build_id)
    {
        flag.store(true, Ordering::SeqCst);
    }
    match store::request_cancel(&state.config.postgres, &row.build_id, &identity.subject).await {
        Ok(updated) => {
            let stages = store::stages(&state.config.postgres, &updated.build_id).await.unwrap_or_default();
            json_ok(202, &build_document("cancelBuild", &updated, &stages))
        }
        Err(error) => json_err(true, &error),
    }
}

async fn get_cards(State(state): State<Arc<AppState>>, headers: HeaderMap, Path(build_id): Path<String>) -> Response {
    let Some(row) = authorized_build(&state, &headers, &build_id, scope::CARDS_READ).await else {
        return auth_failure_or_forbidden(&state, &headers, &build_id, scope::CARDS_READ).await;
    };
    let cards = read_json_file(&state.config.work_dir, &row.build_id, "cards.json").unwrap_or_else(|| json!({"ready": false}));
    json_ok(
        200,
        &json!({
            "contract": CONTRACT,
            "operation": "getBuildCards",
            "build_id": row.build_id,
            "sample_policy": "none",
            "cards": cards,
            "management_identity": "verified",
            "published": false,
            "data_grants": "none"
        }),
    )
}

async fn get_candidates(State(state): State<Arc<AppState>>, headers: HeaderMap, Path(build_id): Path<String>) -> Response {
    let Some(row) = authorized_build(&state, &headers, &build_id, scope::CANDIDATES_READ).await else {
        return auth_failure_or_forbidden(&state, &headers, &build_id, scope::CANDIDATES_READ).await;
    };
    let suggestions = read_json_file(&state.config.work_dir, &row.build_id, "suggestions.json")
        .unwrap_or_else(|| json!({"ready": false, "merged": false, "certified": false}));
    json_ok(
        200,
        &json!({
            "contract": CONTRACT,
            "operation": "getBuildCandidates",
            "build_id": row.build_id,
            "suggestions": suggestions,
            "management_identity": "verified",
            "published": false,
            "data_grants": "none"
        }),
    )
}

async fn list_reviews(State(state): State<Arc<AppState>>, headers: HeaderMap, uri: Uri) -> Response {
    let Some(project_id) = query_value(&uri, "project_id") else {
        return json_err(false, &ApiError::malformed());
    };
    let Some(build_id) = query_value(&uri, "build_id") else {
        return json_err(false, &ApiError::malformed());
    };
    if gate_read(&state, &headers, &project_id, scope::REVIEW_READ).await.is_none() {
        return gate_read_response(&state, &headers, &project_id, scope::REVIEW_READ).await;
    }
    let items = match store::list_members(&state.config.postgres, &build_id).await {
        Ok(items) => items,
        Err(error) => return json_err(true, &error),
    };
    let visible: Vec<Value> = items
        .into_iter()
        .filter(|item| item.project_id == project_id)
        .map(|item| {
            json!({
                "contract": CONTRACT,
                "operation": "getReviewItem",
                "item_id": item.object_id,
                "project_id": item.project_id,
                "build_id": build_id,
                "candidate_sha256": item.candidate_sha256,
                "source_sha256": item.source_sha256,
                "schema_sha256": item.schema_sha256,
                "evidence_sha256": item.evidence_sha256,
                "management_identity": "verified",
                "ledger_authenticated": false,
                "published": false,
                "data_grants": "none"
            })
        })
        .collect();
    json_ok(
        200,
        &json!({
            "contract": CONTRACT,
            "operation": "listReviewItems",
            "items": visible,
            "management_identity": "verified",
            "published": false,
            "data_grants": "none"
        }),
    )
}

async fn get_review(State(state): State<Arc<AppState>>, headers: HeaderMap, Path(item_id): Path<String>, uri: Uri) -> Response {
    let Some(project_id) = query_value(&uri, "project_id") else {
        return json_err(false, &ApiError::malformed());
    };
    let Some(build_id) = query_value(&uri, "build_id") else {
        return json_err(false, &ApiError::malformed());
    };
    if gate_read(&state, &headers, &project_id, scope::REVIEW_READ).await.is_none() {
        return gate_read_response(&state, &headers, &project_id, scope::REVIEW_READ).await;
    }
    let items = store::list_members(&state.config.postgres, &build_id).await.unwrap_or_default();
    let Some(item) = items.into_iter().find(|item| item.object_id == item_id && item.project_id == project_id) else {
        return json_err(true, &ApiError::forbidden());
    };
    json_ok(
        200,
        &json!({
            "contract": CONTRACT,
            "operation": "getReviewItem",
            "item_id": item.object_id,
            "project_id": item.project_id,
            "build_id": build_id,
            "candidate_sha256": item.candidate_sha256,
            "source_sha256": item.source_sha256,
            "schema_sha256": item.schema_sha256,
            "evidence_sha256": item.evidence_sha256,
            "management_identity": "verified",
            "ledger_authenticated": false,
            "published": false,
            "data_grants": "none"
        }),
    )
}

async fn preview_reviews(State(state): State<Arc<AppState>>, headers: HeaderMap, body: axum::body::Bytes) -> Response {
    let value = match parse_exact(&body) {
        Ok(value) => value,
        Err(error) => return json_err(false, &error),
    };
    if value.get("accept_all") == Some(&Value::Bool(true)) {
        return json_err(false, &ApiError::accept_all());
    }
    let Some(project_id) = value.get("project_id").and_then(Value::as_str) else {
        return json_err(false, &ApiError::malformed());
    };
    let revision = value.get("identity_revision").and_then(Value::as_i64).unwrap_or(0);
    let identity = match open_mutation(&state, &headers, revision).await {
        Ok(identity) => identity,
        Err(error) => return mutation_denied(error),
    };
    let Some(project) = identity.project(project_id) else {
        return json_err(true, &ApiError::forbidden());
    };
    if !allows(&project.capabilities, scope::REVIEW_READ) {
        return json_err(true, &ApiError::forbidden());
    }
    json_ok(
        200,
        &json!({
            "contract": CONTRACT,
            "operation": "previewReviewBatch",
            "wrote": false,
            "items": [],
            "management_identity": "verified",
            "published": false,
            "data_grants": "none"
        }),
    )
}

async fn decide_review(State(state): State<Arc<AppState>>, headers: HeaderMap, Path(item_id): Path<String>, body: axum::body::Bytes) -> Response {
    let value = match parse_exact(&body) {
        Ok(value) => value,
        Err(error) => return json_err(false, &error),
    };
    if membership::reject_forged(&value).is_err() {
        return json_err(false, &ApiError::malformed());
    }
    if value.get("accept_all") == Some(&Value::Bool(true)) {
        return json_err(false, &ApiError::accept_all());
    }
    let revision = value.get("identity_revision").and_then(Value::as_i64).unwrap_or(0);
    let identity = match open_mutation(&state, &headers, revision).await {
        Ok(identity) => identity,
        Err(error) => return mutation_denied(error),
    };
    let Some(build_id) = value.get("build_id").and_then(Value::as_str) else {
        return json_err(true, &ApiError::malformed());
    };
    let Some(row) = load_scoped(&state, &identity, build_id, scope::REVIEW_DECIDE).await else {
        return json_err(true, &ApiError::forbidden());
    };
    let choice = value.get("choice").and_then(Value::as_str).unwrap_or("");
    let kind = value.get("decision_kind").and_then(Value::as_str).unwrap_or("");
    let candidate = value.get("candidate_sha256").and_then(Value::as_str).unwrap_or("");
    if kind != "label_description" || !matches!(choice, "approve" | "reject" | "defer") || !hex64(candidate) {
        return json_err(true, &ApiError::malformed());
    }
    let members = store::list_members(&state.config.postgres, &row.build_id).await.unwrap_or_default();
    let Some(member) = members.into_iter().find(|item| item.object_id == item_id) else {
        return json_err(true, &ApiError::forbidden());
    };
    if !member_matches(&member, &row.project_id, &member.source_sha256, candidate) {
        return json_err(true, &ApiError::membership());
    }
    let Some(expected) = value.get("expected_revision").and_then(Value::as_i64) else {
        return json_err(true, &ApiError::malformed());
    };
    let Some(idempotency_key) = value.get("idempotency_key").and_then(Value::as_str) else {
        return json_err(true, &ApiError::malformed());
    };
    if expected < 1 || !idempotency_ok(idempotency_key) {
        return json_err(true, &ApiError::malformed());
    }
    let fields = value.get("fields").cloned();
    if choice != "approve" && fields.is_some() {
        return json_err(true, &ApiError::malformed());
    }
    if !crate::config::regular_file(&state.config.binaries.review_ledger) {
        return json_err(true, &ApiError::not_ready());
    }
    let body_sha = canonical_sha256(&value);
    match crate::ledger::decide(
        &state.config,
        crate::ledger::DecisionIn {
            operation: "decideReviewItem",
            subject: &identity.subject,
            identity_revision: revision,
            build: &row,
            member: &member,
            expected_revision: Some(expected),
            idempotency_key,
            decision_kind: kind,
            choice,
            fields,
            body_sha256: &body_sha,
        },
    )
    .await
    {
        Ok(document) => json_ok(200, &document),
        Err(error) => json_err(true, &error),
    }
}

async fn list_classifications(State(state): State<Arc<AppState>>, headers: HeaderMap, uri: Uri) -> Response {
    let Some(project_id) = query_value(&uri, "project_id") else {
        return json_err(false, &ApiError::malformed());
    };
    if gate_read(&state, &headers, &project_id, scope::CLASSIFICATION_READ).await.is_none() {
        return gate_read_response(&state, &headers, &project_id, scope::CLASSIFICATION_READ).await;
    }
    let Some(project) = state.config.projects.iter().find(|item| item.project_id == project_id) else {
        return json_err(true, &ApiError::forbidden());
    };
    let columns: Vec<Value> = project
        .visible_columns
        .iter()
        .filter(|column| !project.hidden_columns.iter().any(|hidden| hidden == *column))
        .map(|column| json!({"object_id": column, "hidden": false}))
        .collect();
    json_ok(
        200,
        &json!({
            "contract": CONTRACT,
            "operation": "listClassifications",
            "columns": columns,
            "management_identity": "verified",
            "published": false,
            "data_grants": "none"
        }),
    )
}

async fn preview_classes(State(state): State<Arc<AppState>>, headers: HeaderMap, body: axum::body::Bytes) -> Response {
    class_body(&state, &headers, &body, false).await
}

async fn confirm_classes(State(state): State<Arc<AppState>>, headers: HeaderMap, body: axum::body::Bytes) -> Response {
    class_body(&state, &headers, &body, true).await
}

async fn class_body(state: &AppState, headers: &HeaderMap, body: &[u8], confirm: bool) -> Response {
    let value = match parse_exact(body) {
        Ok(value) => value,
        Err(error) => return json_err(false, &error),
    };
    let accept_all = value.get("accept_all") == Some(&Value::Bool(true));
    let Some(project_id) = value.get("project_id").and_then(Value::as_str) else {
        return json_err(false, &ApiError::malformed());
    };
    let revision = value.get("identity_revision").and_then(Value::as_i64).unwrap_or(0);
    let identity = match open_mutation(state, headers, revision).await {
        Ok(identity) => identity,
        Err(error) => return mutation_denied(error),
    };
    let scope_name = if confirm { scope::CLASSIFICATION_CONFIRM } else { scope::CLASSIFICATION_PREVIEW };
    let Some(auth) = identity.project(project_id) else {
        return json_err(true, &ApiError::forbidden());
    };
    if !allows(&auth.capabilities, scope_name) {
        return json_err(true, &ApiError::forbidden());
    }
    let Some(project) = state.config.projects.iter().find(|item| item.project_id == project_id) else {
        return json_err(true, &ApiError::forbidden());
    };
    let mut items = Vec::new();
    if let Some(list) = value.get("items").and_then(Value::as_array) {
        for item in list {
            let Some(object_id) = item.get("object_id").and_then(Value::as_str) else {
                return json_err(true, &ApiError::malformed());
            };
            let Some(pii) = item.get("pii").and_then(Value::as_str) else {
                return json_err(true, &ApiError::malformed());
            };
            items.push((object_id.to_owned(), pii.to_owned()));
        }
    }
    let preview = match preview_classification(&project.visible_columns, &project.hidden_columns, &items, accept_all) {
        Ok(preview) => preview,
        Err(error) => return json_err(true, &error),
    };
    if !confirm {
        return json_ok(
            200,
            &json!({
                "contract": CONTRACT,
                "operation": "previewClassifications",
                "wrote": false,
                "items": preview.ready,
                "management_identity": "verified",
                "published": false,
                "data_grants": "none"
            }),
        );
    }
    let Some(principal) = state.config.principals.iter().find(|principal| principal.subject == identity.subject) else {
        return json_err(true, &ApiError::forbidden());
    };
    if principal.ledger_role != "owner"
        || items.iter().any(|(object_id, _)| !principal.object_scope.iter().any(|scope| scope == object_id))
    {
        return json_err(true, &ApiError::forbidden());
    }
    confirm_stored(state, &identity, project_id, &value).await
}

async fn confirm_stored(state: &AppState, identity: &VerifiedIdentity, project_id: &str, value: &Value) -> Response {
    let Some(build_id) = value.get("build_id").and_then(Value::as_str) else {
        return json_err(true, &ApiError::malformed());
    };
    let Some(idempotency_key) = value.get("idempotency_key").and_then(Value::as_str) else {
        return json_err(true, &ApiError::malformed());
    };
    if !build_id_ok(build_id) || !idempotency_ok(idempotency_key) {
        return json_err(true, &ApiError::malformed());
    }
    let Some(row) = load_scoped_state(state, identity, build_id, scope::CLASSIFICATION_CONFIRM).await else {
        return json_err(true, &ApiError::forbidden());
    };
    if row.project_id != project_id {
        return json_err(true, &ApiError::forbidden());
    }
    let body_sha = canonical_sha256(value);
    match store::confirm_body(&state.config.postgres, project_id, idempotency_key).await {
        Ok(Some(prior)) if prior == body_sha => {
            return json_ok(
                200,
                &json!({
                    "contract": CONTRACT,
                    "operation": "confirmClassifications",
                    "code": "replay",
                    "ledger_authenticated": false,
                    "ledger_management_identity": "not_claimed",
                    "management_identity": "verified",
                    "published": false,
                    "data_grants": "none",
                    "grants": "not_authorized"
                }),
            );
        }
        Ok(Some(_)) => return json_err(true, &ApiError::idempotency()),
        Ok(None) => {}
        Err(error) => return json_err(true, &error),
    }
    let members = match store::list_members(&state.config.postgres, build_id).await {
        Ok(members) => members,
        Err(error) => return json_err(true, &error),
    };
    let Some(list) = value.get("items").and_then(Value::as_array) else {
        return json_err(true, &ApiError::malformed());
    };
    for item in list {
        let Some(object_id) = item.get("object_id").and_then(Value::as_str) else {
            return json_err(true, &ApiError::malformed());
        };
        let Some(pii) = item.get("pii").and_then(Value::as_str) else {
            return json_err(true, &ApiError::malformed());
        };
        let Some(semantic) = item.get("semantic_type").and_then(Value::as_str) else {
            return json_err(true, &ApiError::malformed());
        };
        if !ident_ok(semantic) {
            return json_err(true, &ApiError::malformed());
        }
        let Some(member) = members.iter().find(|member| member.object_id == object_id) else {
            return json_err(true, &ApiError::forbidden());
        };
        let ledger_key = crate::canonical::sha256_hex(format!("{idempotency_key}\n{object_id}").as_bytes());
        if let Err(error) = crate::ledger::decide(
            &state.config,
            crate::ledger::DecisionIn {
                operation: "confirmClassifications",
                subject: &identity.subject,
                identity_revision: value.get("identity_revision").and_then(Value::as_i64).unwrap_or(0),
                build: &row,
                member,
                expected_revision: None,
                idempotency_key: &ledger_key,
                decision_kind: "classification",
                choice: "confirm",
                fields: Some(json!({"pii": pii, "semantic_type": semantic, "state": "confirmed"})),
                body_sha256: &body_sha,
            },
        )
        .await
        {
            return json_err(true, &error);
        }
    }
    if let Err(error) = store::mark_stage(&state.config.postgres, &row, "S10").await {
        return json_err(true, &error);
    }
    if let Err(error) = store::insert_confirm_audit(
        &state.config.postgres,
        project_id,
        build_id,
        &identity.subject,
        idempotency_key,
        &body_sha,
    )
    .await
    {
        return json_err(true, &error);
    }
    json_ok(
        200,
        &json!({
            "contract": CONTRACT,
            "operation": "confirmClassifications",
            "code": "persisted",
            "ledger_authenticated": false,
            "ledger_management_identity": "not_claimed",
            "management_identity": "verified",
            "published": false,
            "data_grants": "none",
            "grants": "not_authorized"
        }),
    )
}

async fn compile_pack(State(state): State<Arc<AppState>>, headers: HeaderMap, Path(build_id): Path<String>, body: axum::body::Bytes) -> Response {
    let value = match parse_exact(&body) {
        Ok(value) => value,
        Err(error) => return json_err(false, &error),
    };
    if membership::reject_forged(&value).is_err() {
        return json_err(false, &ApiError::malformed());
    }
    let revision = value.get("identity_revision").and_then(Value::as_i64).unwrap_or(0);
    let identity = match open_mutation(&state, &headers, revision).await {
        Ok(identity) => identity,
        Err(error) => return mutation_denied(error),
    };
    let Some(idempotency_key) = value.get("idempotency_key").and_then(Value::as_str) else {
        return json_err(true, &ApiError::malformed());
    };
    if !idempotency_ok(idempotency_key) {
        return json_err(true, &ApiError::malformed());
    }
    let Some(row) = load_scoped(&state, &identity, &build_id, scope::PACK_COMPILE).await else {
        return json_err(true, &ApiError::forbidden());
    };
    let body_sha = canonical_sha256(&value);
    match crate::compile::compile(&state.config, &row, &identity.subject, &body_sha).await {
        Ok(document) => json_ok(200, &document),
        Err(error) => json_err(true, &error),
    }
}

async fn get_pack(State(state): State<Arc<AppState>>, headers: HeaderMap, Path(build_id): Path<String>) -> Response {
    if authorized_build(&state, &headers, &build_id, scope::PACK_READ).await.is_none() {
        return auth_failure_or_forbidden(&state, &headers, &build_id, scope::PACK_READ).await;
    }
    match crate::compile::load_stored(&state.config, &build_id).await {
        Ok(document) => json_ok(200, &document),
        Err(error) => json_err(true, &error),
    }
}

async fn read_build(state: &Arc<AppState>, headers: &HeaderMap, build_id: &str, scope_name: &str, operation: &str) -> Response {
    let Some(row) = authorized_build(state, headers, build_id, scope_name).await else {
        return auth_failure_or_forbidden(state, headers, build_id, scope_name).await;
    };
    let stages = store::stages(&state.config.postgres, &row.build_id).await.unwrap_or_default();
    json_ok(200, &build_document(operation, &row, &stages))
}

async fn authorized_build(state: &AppState, headers: &HeaderMap, build_id: &str, scope_name: &str) -> Option<BuildRow> {
    let cookie = session_cookie(headers)?;
    let identity = fetch_state(state, &cookie).await.ok()?;
    load_scoped_state(state, &identity, build_id, scope_name).await
}

async fn auth_failure_or_forbidden(state: &AppState, headers: &HeaderMap, build_id: &str, scope_name: &str) -> Response {
    let Some(cookie) = session_cookie(headers) else {
        return json_err(false, &ApiError::unauthenticated());
    };
    let identity = match fetch_state(state, &cookie).await {
        Ok(identity) => identity,
        Err(error) => return json_err(false, &error),
    };
    let _ = (build_id, scope_name, identity);
    json_err(true, &ApiError::forbidden())
}

async fn gate_read(state: &AppState, headers: &HeaderMap, project_id: &str, scope_name: &str) -> Option<VerifiedIdentity> {
    let cookie = session_cookie(headers)?;
    let identity = fetch_state(state, &cookie).await.ok()?;
    let project = identity.project(project_id)?;
    if allows(&project.capabilities, scope_name) {
        Some(identity)
    } else {
        None
    }
}

async fn gate_read_response(state: &AppState, headers: &HeaderMap, project_id: &str, scope_name: &str) -> Response {
    let Some(cookie) = session_cookie(headers) else {
        return json_err(false, &ApiError::unauthenticated());
    };
    match fetch_state(state, &cookie).await {
        Ok(identity) => {
            let allowed = identity.project(project_id).is_some_and(|project| allows(&project.capabilities, scope_name));
            if allowed {
                json_err(true, &ApiError::not_ready())
            } else {
                json_err(true, &ApiError::forbidden())
            }
        }
        Err(error) => json_err(false, &error),
    }
}

async fn load_scoped(state: &AppState, identity: &VerifiedIdentity, build_id: &str, scope_name: &str) -> Option<BuildRow> {
    load_scoped_state(state, identity, build_id, scope_name).await
}

async fn load_scoped_state(state: &AppState, identity: &VerifiedIdentity, build_id: &str, scope_name: &str) -> Option<BuildRow> {
    if !build_id_ok(build_id) {
        return None;
    }
    let row = store::load_build_any(&state.config.postgres, build_id).await.ok()??;
    let project = identity.project(&row.project_id)?;
    if allows(&project.capabilities, scope_name) {
        Some(row)
    } else {
        None
    }
}

async fn open_mutation(state: &AppState, headers: &HeaderMap, revision: i64) -> Result<VerifiedIdentity, ApiError> {
    let Some(cookie) = session_cookie(headers) else {
        return Err(ApiError::unauthenticated());
    };
    let ca = identity::load_ca(&state.config.identity.ca_file)?;
    identity::authorize_mutation(
        &state.config.identity.origin,
        &ca,
        &cookie,
        header_str(headers, "origin"),
        header_str(headers, "x-oo-csrf"),
        &state.config.deployment_origins,
        revision,
    )
    .await
}

fn mutation_denied(error: ApiError) -> Response {
    json_err(error.code == "revision.conflict", &error)
}

async fn fetch_state(state: &AppState, cookie: &str) -> Result<VerifiedIdentity, ApiError> {
    let ca = identity::load_ca(&state.config.identity.ca_file)?;
    identity::fetch_current(&state.config.identity.origin, &ca, cookie)
        .await
        .map_err(crate::identity::IdentityFault::api)
}

struct CreateFields {
    project_id: String,
    revision: i64,
    config_commit: String,
    source_ref: String,
    allowance_id: String,
    llm_secret_ref: String,
    idempotency_key: String,
}

fn create_fields(value: &Value) -> Result<CreateFields, ApiError> {
    let object = value.as_object().ok_or(ApiError::malformed())?;
    if object.len() != 7 {
        return Err(ApiError::malformed());
    }
    let project_id = required_str(object, "project_id")?;
    let config_commit = required_str(object, "config_commit")?;
    let source_ref = required_str(object, "source_ref")?;
    let allowance_id = required_str(object, "allowance_id")?;
    let llm_secret_ref = required_str(object, "llm_secret_ref")?;
    let idempotency_key = required_str(object, "idempotency_key")?;
    let revision = object.get("identity_revision").and_then(Value::as_i64).ok_or(ApiError::malformed())?;
    if !ident_ok(&project_id)
        || !ident_ok(&source_ref)
        || !ident_ok(&allowance_id)
        || !hex64(&config_commit)
        || !secret_ref_ok(&llm_secret_ref)
        || !idempotency_ok(&idempotency_key)
        || revision < 1
        || object.get("identity_revision").and_then(Value::as_u64).is_none()
    {
        return Err(ApiError::malformed());
    }
    Ok(CreateFields {
        project_id,
        revision,
        config_commit,
        source_ref,
        allowance_id,
        llm_secret_ref,
        idempotency_key,
    })
}

fn required_str(object: &serde_json::Map<String, Value>, key: &str) -> Result<String, ApiError> {
    object
        .get(key)
        .and_then(Value::as_str)
        .map(str::to_owned)
        .ok_or(ApiError::malformed())
}

fn build_document(operation: &str, row: &BuildRow, stages: &[StageRow]) -> Value {
    let stage_values: Vec<Value> = stages
        .iter()
        .map(|stage| {
            json!({
                "stage": stage.stage,
                "status": stage.status,
                "output_sha256": stage.output_sha256,
                "detail_code": stage.detail_code
            })
        })
        .collect();
    json!({
        "contract": CONTRACT,
        "operation": operation,
        "build_id": row.build_id,
        "project_id": row.project_id,
        "status": row.status,
        "stage": row.stage,
        "identity_revision": row.identity_revision,
        "input_sha256": row.input_sha256,
        "failure_code": row.failure_code,
        "stages": stage_values,
        "management_identity": "verified",
        "published": false,
        "active": false,
        "data_grants": "none",
        "compiled_pack": "not_published"
    })
}

fn read_json_file(work_dir: &str, build_id: &str, name: &str) -> Option<Value> {
    let path = std::path::Path::new(work_dir).join("builds").join(build_id).join(name);
    let bytes = std::fs::read(path).ok()?;
    serde_json::from_slice(&bytes).ok()
}

fn session_cookie(headers: &HeaderMap) -> Option<String> {
    let raw = headers.get("cookie")?.to_str().ok()?;
    for part in raw.split(';') {
        let part = part.trim();
        let Some(value) = part.strip_prefix("__Host-oo-session=") else {
            continue;
        };
        if session_cookie_ok(value) {
            return Some(value.to_owned());
        }
        return None;
    }
    None
}

fn header_str<'a>(headers: &'a HeaderMap, name: &str) -> Option<&'a str> {
    headers.get(name).and_then(|value| value.to_str().ok())
}

fn query_value(uri: &Uri, key: &str) -> Option<String> {
    let query = uri.query()?;
    for pair in query.split('&') {
        let (name, value) = pair.split_once('=')?;
        if name == key && !value.is_empty() {
            return Some(value.to_owned());
        }
    }
    None
}

fn json_ok(status: u16, value: &Value) -> Response {
    json_response(status, value)
}

fn json_err(verified: bool, error: &ApiError) -> Response {
    json_response(error.status, &error.body(verified))
}

fn json_response(status: u16, value: &Value) -> Response {
    let bytes = serde_json::to_vec(value).unwrap_or_else(|_| b"{}".to_vec());
    Response::builder()
        .status(status)
        .header("content-type", "application/json")
        .header("cache-control", "no-store")
        .body(Body::from(bytes))
        .unwrap_or_else(|_| Response::new(Body::empty()))
}
