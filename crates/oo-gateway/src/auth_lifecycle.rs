//! HTTP adaptation: public requests never choose a project, audience or actor.
use super::token_lifecycle::{AdminRequest, Control, DAY, Operation, Receipt};
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
        .route("/v1/health", get(expiry_health))
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
pub fn expiry(deadline: Option<i64>, now: i64) -> Value {
    match deadline {
        None => serde_json::json!({"expires_at_ms":null,"status":"unmeasured"}),
        Some(at) => {
            serde_json::json!({"expires_at_ms":at,"remaining_ms":at.saturating_sub(now),"status":if at<=now {"expired"}else if at.saturating_sub(now)<=14*DAY {"warning"}else{"ok"}})
        }
    }
}
async fn expiry_health(
    State(state): State<AppState>,
    peer: PublicPeer,
    RawQuery(query): RawQuery,
    headers: HeaderMap,
) -> Response {
    let caller = match public_caller(&state, &headers, &query, peer).await {
        Ok(c) => c,
        Err(r) => return *r,
    };
    let endpoint = match read_endpoint(&state.inner.endpoint_path) {
        Ok(e) => e,
        Err(_) => return failure("database"),
    };
    let client = match connect(&state, &endpoint).await {
        Ok(c) => c,
        Err(_) => return failure("database"),
    };
    let row=client.query_one("SELECT min(t.absolute_deadline_ms) FROM oo_token_control.token_record t WHERE t.project_id=$1 AND t.audience_url=$2 AND NOT EXISTS (SELECT 1 FROM oo_token_control.revocation r WHERE r.project_id=t.project_id AND ((r.scope='token' AND r.token_sha256=t.token_sha256) OR (r.seq>t.revocation_floor AND (r.scope='project' OR (r.scope='principal' AND r.principal_id=t.principal_id) OR (r.scope='session' AND r.session_id=t.session_id)))))",&[&caller.tuple.project_id,&state.inner.audience]).await;
    let Ok(row) = row else {
        return failure("database");
    };
    let Ok(now) = token_lifecycle::now_ms() else {
        return failure("clock");
    };
    json_value(
        200,
        serde_json::json!({"token_expiry":expiry(row.get(0),now),"internal_mtls_certificate_expiry":expiry(state.inner.certificate_expiry_ms,now)}),
    )
}

// Read the certificate's signed notAfter, not an operator-supplied expiry claim.
pub(super) fn certificate_expiry(der: &[u8]) -> Result<i64, &'static str> {
    let mut outer = Der::new(der);
    let mut cert = Der::new(outer.expect(0x30).map_err(|_| "certificate")?);
    let mut tbs = Der::new(cert.expect(0x30).map_err(|_| "certificate")?);
    if tbs.rest.first() == Some(&0xa0) {
        tbs.expect(0xa0).map_err(|_| "certificate")?;
    }
    for tag in [0x02, 0x30, 0x30] {
        tbs.expect(tag).map_err(|_| "certificate")?;
    }
    let mut validity = Der::new(tbs.expect(0x30).map_err(|_| "certificate")?);
    let tag = *validity.rest.first().ok_or("certificate")?;
    validity.expect(tag).map_err(|_| "certificate")?;
    let tag = *validity.rest.first().ok_or("certificate")?;
    let bytes = validity.expect(tag).map_err(|_| "certificate")?;
    parse_time(tag, bytes)
}
fn parse_time(tag: u8, bytes: &[u8]) -> Result<i64, &'static str> {
    let (year, offset) = match tag {
        0x17 if bytes.len() == 13 => {
            let y = digits(&bytes[..2])?;
            (if y >= 50 { 1900 + y } else { 2000 + y }, 2)
        }
        0x18 if bytes.len() == 15 => (digits(&bytes[..4])?, 4),
        _ => return Err("certificate"),
    };
    if bytes.last() != Some(&b'Z') {
        return Err("certificate");
    }
    let month = digits(&bytes[offset..offset + 2])?;
    let day = digits(&bytes[offset + 2..offset + 4])?;
    let hour = digits(&bytes[offset + 4..offset + 6])?;
    let minute = digits(&bytes[offset + 6..offset + 8])?;
    let second = digits(&bytes[offset + 8..offset + 10])?;
    let leap = year % 4 == 0 && (year % 100 != 0 || year % 400 == 0);
    let lengths = [
        31,
        if leap { 29 } else { 28 },
        31,
        30,
        31,
        30,
        31,
        31,
        30,
        31,
        30,
        31,
    ];
    if !(1..=12).contains(&month)
        || day < 1
        || day > lengths[(month - 1) as usize]
        || hour > 23
        || minute > 59
        || second > 59
    {
        return Err("certificate");
    }
    let mut days = 0i64;
    for y in 1970..year {
        days += if y % 4 == 0 && (y % 100 != 0 || y % 400 == 0) {
            366
        } else {
            365
        };
    }
    for y in year..1970 {
        days -= if y % 4 == 0 && (y % 100 != 0 || y % 400 == 0) {
            366
        } else {
            365
        };
    }
    days += lengths[..(month - 1) as usize].iter().sum::<i64>() + day - 1;
    Ok(((days * 24 + hour) * 3600 + minute * 60 + second) * 1000)
}
fn digits(bytes: &[u8]) -> Result<i64, &'static str> {
    if !bytes.iter().all(u8::is_ascii_digit) {
        return Err("certificate");
    }
    Ok(bytes.iter().fold(0, |n, b| n * 10 + i64::from(b - b'0')))
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn warning_boundaries_and_certificate_dates() {
        assert_eq!(expiry(Some(14 * DAY), 0)["status"], "warning");
        assert_eq!(expiry(Some(14 * DAY + 1), 0)["status"], "ok");
        assert_eq!(expiry(Some(0), 0)["status"], "expired");
        assert_eq!(expiry(None, 0)["status"], "unmeasured");
        assert_eq!(parse_time(0x17, b"700101000000Z"), Ok(0));
        assert_eq!(parse_time(0x18, b"20261104000000Z"), Ok(1793750400000));
        assert!(parse_time(0x18, b"20260229000000Z").is_err());
    }
}
