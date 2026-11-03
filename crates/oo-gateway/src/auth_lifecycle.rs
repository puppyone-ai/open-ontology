//! HTTP adaptation: public requests never choose a project, audience or actor.
use super::token_lifecycle::{AdminRequest, Control, Receipt};
use super::*;

pub(super) struct Lifecycle {
    pub control: Control,
    pub admin_workload: String,
}
impl Lifecycle {
    pub fn new(control: Control, admin_workload: String) -> Self {
        Self {
            control,
            admin_workload,
        }
    }
}
pub(super) fn mount(router: Router<AppState>) -> Router<AppState> {
    router.route("/internal/v1/tokens", post(admin))
}
fn failure(reason: &str) -> Response {
    let status = match reason {
        "database" | "connect" | "audit" | "commit_unknown" | "install_drift" => 503,
        "rate_limit" => 429,
        "forbidden" => 403,
        "unauthenticated" | "tls_required" => 401,
        _ => 400,
    };
    let mut response = json_value(
        status,
        serde_json::json!({"error":{"code":"TOKEN_LIFECYCLE","message":reason}}),
    );
    response
        .headers_mut()
        .insert(header::CACHE_CONTROL, HeaderValue::from_static("no-store"));
    response
}
fn receipt(result: Result<Receipt, &'static str>) -> Response {
    match result {
        Err(reason) => failure(reason),
        Ok(mut receipt) => {
            if let Some(secret) = receipt.secret.take() {
                let key = if secret.starts_with("OO-") {
                    "code"
                } else {
                    "token"
                };
                receipt.metadata[key] = Value::String(secret.to_string());
            }
            let mut response = json_value(200, receipt.metadata);
            response
                .headers_mut()
                .insert(header::CACHE_CONTROL, HeaderValue::from_static("no-store"));
            response
        }
    }
}
fn tls_gate(
    state: &AppState,
    peer: &PublicPeer,
    query: &Option<String>,
    headers: &HeaderMap,
) -> bool {
    matches!(state.inner.transport, Transport::Tls { .. })
        && peer.is_some()
        && query.is_none()
        && !forged_certificate_header(headers)
}
async fn admin(
    State(state): State<AppState>,
    peer: PublicPeer,
    RawQuery(query): RawQuery,
    headers: HeaderMap,
    body: Bytes,
) -> Response {
    if !tls_gate(&state, &peer, &query, &headers) {
        return failure("tls_required");
    }
    let Some(lifecycle) = &state.inner.lifecycle else {
        return failure("disabled");
    };
    let Some(Extension(ConnectInfo(peer))) = peer else {
        return failure("forbidden");
    };
    if peer.workload.is_empty() || peer.workload != lifecycle.admin_workload {
        return failure("forbidden");
    }
    let Ok(request) = serde_json::from_slice::<AdminRequest>(&body) else {
        return failure("input");
    };
    receipt(
        lifecycle
            .control
            .execute(request.into(), &peer.workload)
            .await,
    )
}
