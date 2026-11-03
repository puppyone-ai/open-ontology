//! HTTP adaptation: public requests never choose a project, audience or actor.
use super::token_lifecycle::{AdminRequest, Control, Operation, Receipt};
use super::*;
use zeroize::Zeroizing;

pub(super) struct Lifecycle {
    pub control: Control,
    pub admin_workload: String,
    // Global enrollment limiter bounds guessing even if source addresses are spoofed.
    attempts: Mutex<(Instant, u32)>,
}
impl Lifecycle {
    pub fn new(control: Control, admin_workload: String) -> Self {
        Self {
            control,
            admin_workload,
            attempts: Mutex::new((Instant::now(), 0)),
        }
    }
    fn admit(&self) -> bool {
        let Ok(mut state) = self.attempts.lock() else {
            return false;
        };
        if state.0.elapsed() >= Duration::from_secs(60) {
            *state = (Instant::now(), 0);
        }
        if state.1 >= 10 {
            return false;
        }
        state.1 += 1;
        true
    }
}
pub(super) fn mount(router: Router<AppState>) -> Router<AppState> {
    router
        .route("/v1/auth/enroll", post(enroll))
        .route("/v1/auth/renew", post(renew))
        .route("/internal/v1/tokens", post(admin))
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
async fn enroll(
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
    if !lifecycle.admit() {
        return failure("rate_limit");
    }
    #[derive(serde::Deserialize)]
    #[serde(deny_unknown_fields)]
    struct Input {
        code: String,
    }
    let Ok(input) = serde_json::from_slice::<Input>(&body) else {
        return failure("input");
    };
    let code = Zeroizing::new(input.code);
    if code.len() != 12 || !code.starts_with("OO-") || code.as_bytes()[7] != b'-' {
        return failure("input");
    }
    receipt(
        lifecycle
            .control
            .execute(
                Operation::Redeem {
                    code,
                    audience: state.inner.audience.clone(),
                },
                "public-enrollment",
            )
            .await,
    )
}
async fn renew(
    State(state): State<AppState>,
    peer: PublicPeer,
    RawQuery(query): RawQuery,
    headers: HeaderMap,
    body: Bytes,
) -> Response {
    if !tls_gate(&state, &peer, &query, &headers) {
        return failure("tls_required");
    }
    if !body.is_empty() && body.as_ref() != b"{}" {
        return failure("input");
    }
    let Some(lifecycle) = &state.inner.lifecycle else {
        return failure("disabled");
    };
    let token = match bearer(&headers) {
        Ok(token) => Zeroizing::new(token),
        Err(_) => return failure("unauthenticated"),
    };
    receipt(
        lifecycle
            .control
            .execute(
                Operation::SelfRenew {
                    token,
                    audience: state.inner.audience.clone(),
                },
                "token-holder",
            )
            .await,
    )
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
