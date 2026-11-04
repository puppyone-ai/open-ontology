//! HTTP adaptation: public requests never choose a project, audience or actor.
use super::token_lifecycle::{AdminRequest, Control, DAY, Operation, Receipt};
use super::*;
use zeroize::Zeroizing;

#[path = "auth_device.rs"]
mod device;

pub(super) struct Lifecycle {
    pub control: Control,
    pub admin_workload: String,
    attempts: Mutex<Attempts>,
    devices: Mutex<device::Devices>,
    unknown_device_polls: std::sync::atomic::AtomicU64,
}
impl Lifecycle {
    pub fn new(control: Control, admin_workload: String) -> Self {
        Self {
            control,
            admin_workload,
            attempts: Mutex::new(Attempts::new()),
            devices: Mutex::new(device::Devices::default()),
            unknown_device_polls: std::sync::atomic::AtomicU64::new(0),
        }
    }
    fn admit(&self, route: &'static str, peer: &PublicPeer) -> bool {
        self.attempts
            .lock()
            .is_ok_and(|mut attempts| attempts.admit(route, peer_key(peer)))
    }
    fn admit_renew(&self, token: &str) -> bool {
        let Ok((_, hash)) = token_hash(token) else {
            return false;
        };
        self.attempts
            .lock()
            .is_ok_and(|mut attempts| attempts.admit("renew", hash))
    }
}
fn peer_key(peer: &PublicPeer) -> String {
    peer.as_ref()
        .map(|Extension(ConnectInfo(p))| {
            p.remote_ip
                .map(|ip| ip.to_string())
                .unwrap_or_else(|| p.workload.clone())
        })
        .unwrap_or_default()
}
fn audit_actor(route: &str, peer: &PublicPeer) -> String {
    use sha2::{Digest, Sha256};
    let workload = peer
        .as_ref()
        .map(|Extension(ConnectInfo(p))| p.workload.as_str())
        .unwrap_or("");
    // Hash the verified workload, not headers or supplied redemption metadata.
    format!(
        "{route};peer={};workload_sha256={:x}",
        peer.as_ref()
            .and_then(|Extension(ConnectInfo(p))| p.remote_ip)
            .map(|ip| ip.to_string())
            .unwrap_or_else(|| "unmeasured".into()),
        Sha256::digest(workload.as_bytes())
    )
}
struct Attempts {
    start: Instant,
    total: u32,
    routes: HashMap<&'static str, u32>,
    peers: HashMap<(&'static str, String), u32>,
}
impl Attempts {
    fn new() -> Self {
        Self {
            start: Instant::now(),
            total: 0,
            routes: HashMap::new(),
            peers: HashMap::new(),
        }
    }
    fn admit(&mut self, route: &'static str, peer: String) -> bool {
        if self.start.elapsed() >= Duration::from_secs(60) {
            *self = Self::new();
        }
        let key = (route, peer);
        // Bound memory even for authenticated callers with many distinct tokens.
        if !self.peers.contains_key(&key) && self.peers.len() >= 4096 {
            return false;
        }
        let keyed_limit = match route {
            "renew" => Some(10),
            "device_poll" => Some(30),
            "device_grant" => Some(12),
            _ => None,
        };
        if let Some(limit) = keyed_limit {
            let count = self.peers.entry(key).or_default();
            if *count >= limit {
                return false;
            }
            *count += 1;
            return true;
        }
        if self.total >= 90
            || self.routes.get(route).copied().unwrap_or(0) >= 10
            || self.peers.get(&key).copied().unwrap_or(0) >= 5
        {
            return false;
        }
        self.total += 1;
        *self.routes.entry(route).or_default() += 1;
        *self.peers.entry(key).or_default() += 1;
        true
    }
}
pub(super) fn mount(router: Router<AppState>) -> Router<AppState> {
    router
        .route("/v1/auth/enroll", post(enroll))
        .route("/v1/auth/renew", post(renew))
        .route("/v1/auth/whoami", get(whoami))
        .route("/v1/auth/device/code", post(device::authorize))
        .route("/v1/auth/device/token", post(device::poll))
        .route("/v1/auth/device", get(device::verification))
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
    if !lifecycle.admit("enroll", &peer) {
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
                &audit_actor("public-enrollment", &peer),
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
    // Fresh runtime-role lookup precedes every privileged connection and write.
    if !matches!(authenticate_token(&state, &token, true).await,
        Ok(identity) if identity.scope == "client")
    {
        return failure("unauthenticated");
    }
    if !lifecycle.admit_renew(&token) {
        return failure("rate_limit");
    }
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
    if serde_json::from_slice::<Value>(&body)
        .ok()
        .is_some_and(|v| v["operation"] == "device_approve")
    {
        return device::approve(&state, &body, &peer.workload).await;
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

async fn whoami(
    State(state): State<AppState>,
    peer: PublicPeer,
    RawQuery(query): RawQuery,
    headers: HeaderMap,
) -> Response {
    let caller = match public_caller(&state, &headers, &query, peer).await {
        Ok(caller) => caller,
        Err(response) => return *response,
    };
    let hash = match bearer(&headers).and_then(|token| token_hash(&token).map_err(|_| ())) {
        Ok((_, hash)) => hash,
        Err(_) => return failure("unauthenticated"),
    };
    let endpoint = match read_endpoint(&state.inner.endpoint_path) {
        Ok(endpoint) => endpoint,
        Err(_) => return failure("database"),
    };
    let client = match connect(&state, &endpoint).await {
        Ok(client) => client,
        Err(_) => return failure("database"),
    };
    let row = match client.query_one(READ_TOKEN, &[&hash]).await {
        Ok(row) => row,
        Err(_) => return failure("unauthenticated"),
    };
    let absolute: i64 = row.get(12);
    let idle: Option<i64> = row.get(11);
    let mut response = json_value(
        200,
        serde_json::json!({
            "project_id": caller.tuple.project_id,
            "principal_id": caller.tuple.principal_id,
            "role": caller.tuple.role,
            "expires_at_ms": idle.map_or(absolute, |at| at.min(absolute)),
            "protocol_version": "0.2",
        }),
    );
    response
        .headers_mut()
        .insert(header::CACHE_CONTROL, HeaderValue::from_static("no-store"));
    response
}
pub fn expiry(deadline: Option<i64>, now: i64) -> Value {
    match deadline {
        None => serde_json::json!({"expires_at_ms":null,"status":"unmeasured"}),
        Some(at) => {
            serde_json::json!({"expires_at_ms":at,"remaining_ms":at.saturating_sub(now),"status":if at<=now {"expired"}else if at.saturating_sub(now)<=14*DAY {"warning"}else{"ok"}})
        }
    }
}
const HEALTH_EXPIRY_SQL: &str = "SELECT min(t.absolute_deadline_ms) FROM oo_token_control.token_record t LEFT JOIN oo_token_lifecycle.token x USING(token_sha256) WHERE t.project_id=$1 AND t.audience_url=$2 AND t.issued_at_ms <= $3 AND t.absolute_deadline_ms > $3 AND (t.idle_deadline_ms IS NULL OR t.idle_deadline_ms > $3) AND (x.overlap_until_ms IS NULL OR x.overlap_until_ms > $3) AND NOT EXISTS (SELECT 1 FROM oo_token_control.revocation r WHERE r.project_id=t.project_id AND ((r.scope='token' AND r.token_sha256=t.token_sha256) OR (r.seq>t.revocation_floor AND (r.scope='project' OR (r.scope='principal' AND r.principal_id=t.principal_id) OR (r.scope='session' AND r.session_id=t.session_id)))))";
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
    let Ok(now) = token_lifecycle::now_ms() else {
        return failure("clock");
    };
    let row = client
        .query_one(
            HEALTH_EXPIRY_SQL,
            &[&caller.tuple.project_id, &state.inner.audience, &now],
        )
        .await;
    let Ok(row) = row else {
        return failure("database");
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
    fn lifecycle_state() -> AppState {
        let mut state = AppState::for_limits(PackCatalog::empty(), 1024);
        let inner = Arc::get_mut(&mut state.inner).unwrap();
        let config = rustls::ServerConfig::builder_with_provider(Arc::new(
            rustls::crypto::aws_lc_rs::default_provider(),
        ))
        .with_safe_default_protocol_versions()
        .unwrap()
        .with_no_client_auth()
        .with_cert_resolver(Arc::new(rustls::server::ResolvesServerCertUsingSni::new()));
        inner.transport = Transport::Tls {
            acceptor: TlsAcceptor::from(Arc::new(config)),
            handshake_timeout: Duration::from_secs(1),
        };
        inner.lifecycle = Some(Lifecycle::new(
            Control::new(
                Endpoint {
                    host: "localhost".into(),
                    port: 1,
                    database: "unused".into(),
                },
                String::new(),
                Vec::new(),
            ),
            "test-admin".into(),
        ));
        state
    }
    #[test]
    fn renewal_buckets_do_not_starve_public_routes() {
        let mut attempts = Attempts::new();
        for route in ["enroll", "device_code", "device_poll"] {
            let limit = if route == "device_poll" { 60 } else { 10 };
            for n in 0..limit {
                assert!(attempts.admit(route, n.to_string()));
            }
        }
        for n in 0..20 {
            for _ in 0..10 {
                assert!(attempts.admit("renew", format!("token-{n}")));
            }
            assert!(!attempts.admit("renew", format!("token-{n}")));
        }
        assert_eq!(attempts.total, 20);
        attempts.start -= Duration::from_secs(60);
        assert!(attempts.admit("renew", "new-peer".into()));
        assert_eq!(attempts.peers.len(), 1);
    }
    #[test]
    fn device_poll_budgets_are_per_peer_and_per_code() {
        let mut attempts = Attempts::new();
        for peer in 0..10 {
            for _ in 0..30 {
                assert!(attempts.admit("device_poll", peer.to_string()));
            }
            assert!(!attempts.admit("device_poll", peer.to_string()));
        }
        for code in 0..10 {
            for _ in 0..12 {
                assert!(attempts.admit("device_grant", code.to_string()));
            }
            assert!(!attempts.admit("device_grant", code.to_string()));
        }
        assert!(attempts.admit("enroll", "one".into()));
        attempts.start -= Duration::from_secs(60);
        assert!(attempts.admit("device_poll", "0".into()));
        assert!(attempts.admit("device_grant", "0".into()));
    }
    fn test_peer(workload: &str) -> PublicPeer {
        Some(Extension(ConnectInfo(TlsPeer {
            workload: workload.into(),
            remote_ip: match workload {
                "one" => Some("192.0.2.1".parse().unwrap()),
                "two" => Some("192.0.2.2".parse().unwrap()),
                "three" => Some("192.0.2.3".parse().unwrap()),
                _ => None,
            },
        })))
    }
    #[test]
    fn peer_address_buckets_ignore_workload_changes() {
        let mut peer = test_peer("one");
        let key = peer_key(&peer);
        peer.as_mut().unwrap().0.0.workload = "another-workload".into();
        assert_eq!(peer_key(&peer), key);
        assert!(audit_actor("device-code", &peer).contains("peer=192.0.2.1;"));
        assert_ne!(peer_key(&test_peer("two")), key);
    }
    #[tokio::test]
    async fn route_and_peer_budgets_are_independent() {
        let state = lifecycle_state();
        for _ in 0..5 {
            assert_eq!(
                enroll(
                    State(state.clone()),
                    test_peer("one"),
                    RawQuery(None),
                    HeaderMap::new(),
                    Bytes::from_static(b"{}")
                )
                .await
                .status(),
                StatusCode::BAD_REQUEST
            );
        }
        assert_eq!(
            enroll(
                State(state.clone()),
                test_peer("one"),
                RawQuery(None),
                HeaderMap::new(),
                Bytes::from_static(b"{}")
            )
            .await
            .status(),
            StatusCode::TOO_MANY_REQUESTS
        );
        for _ in 0..5 {
            assert_eq!(
                enroll(
                    State(state.clone()),
                    test_peer("two"),
                    RawQuery(None),
                    HeaderMap::new(),
                    Bytes::from_static(b"{}")
                )
                .await
                .status(),
                StatusCode::BAD_REQUEST
            );
        }
        assert_eq!(
            enroll(
                State(state.clone()),
                test_peer("three"),
                RawQuery(None),
                HeaderMap::new(),
                Bytes::from_static(b"{}")
            )
            .await
            .status(),
            StatusCode::TOO_MANY_REQUESTS
        );
        let mut headers = HeaderMap::new();
        headers.insert(
            header::CONTENT_TYPE,
            HeaderValue::from_static("application/x-www-form-urlencoded"),
        );
        assert_eq!(
            device::authorize(
                State(state.clone()),
                test_peer("one"),
                RawQuery(None),
                headers,
                Bytes::from_static(b"client_id=oo-cli")
            )
            .await
            .status(),
            StatusCode::BAD_REQUEST
        );
    }
    #[tokio::test]
    async fn device_code_requires_a_durable_audit_before_disclosure() {
        let state = lifecycle_state();
        let mut headers = HeaderMap::new();
        headers.insert(
            header::CONTENT_TYPE,
            HeaderValue::from_static("application/x-www-form-urlencoded"),
        );
        let response = device::authorize(
            State(state.clone()),
            test_peer("device"),
            RawQuery(None),
            headers,
            Bytes::from_static(b"client_id=oo-cli"),
        )
        .await;
        let body = axum::body::to_bytes(response.into_body(), 4096)
            .await
            .unwrap();
        assert_eq!(
            serde_json::from_slice::<Value>(&body).unwrap()["error"],
            "server_error"
        );
        assert_eq!(
            state
                .inner
                .lifecycle
                .as_ref()
                .unwrap()
                .control
                .connections
                .load(Ordering::SeqCst),
            1
        );
    }
    #[tokio::test]
    async fn unknown_device_poll_never_opens_admin_connection() {
        let state = lifecycle_state();
        let mut headers = HeaderMap::new();
        headers.insert(
            header::CONTENT_TYPE,
            HeaderValue::from_static("application/x-www-form-urlencoded"),
        );
        let response = device::poll(State(state.clone()), test_peer("device"), RawQuery(None), headers,
            Bytes::from(format!("client_id=oo-cli&grant_type=urn:ietf:params:oauth:grant-type:device_code&device_code={}", "a".repeat(64)))).await;
        let body = axum::body::to_bytes(response.into_body(), 4096)
            .await
            .unwrap();
        assert_eq!(
            serde_json::from_slice::<Value>(&body).unwrap()["error"],
            "expired_token"
        );
        assert_eq!(
            state
                .inner
                .lifecycle
                .as_ref()
                .unwrap()
                .control
                .connections
                .load(Ordering::SeqCst),
            0
        );
        assert_eq!(
            state
                .inner
                .lifecycle
                .as_ref()
                .unwrap()
                .unknown_device_polls
                .load(Ordering::Relaxed),
            1
        );
    }
    #[test]
    fn audit_actor_uses_only_verified_workload_and_marks_missing_peer() {
        use sha2::{Digest, Sha256};
        let actor = audit_actor("public-enrollment", &test_peer("verified-workload"));
        assert!(actor.contains("peer=unmeasured"));
        assert!(actor.ends_with(&format!("{:x}", Sha256::digest(b"verified-workload"))));
        assert!(!actor.contains("verified-workload"));
    }
    #[tokio::test]
    async fn junk_renew_never_opens_admin_connection() {
        let state = lifecycle_state();
        for material in ["junk".to_string(), format!("ooct_{}", "A".repeat(43))] {
            let mut headers = HeaderMap::new();
            headers.insert(
                header::AUTHORIZATION,
                format!("Bearer {material}").parse().unwrap(),
            );
            let response = renew(
                State(state.clone()),
                Some(Extension(ConnectInfo(TlsPeer {
                    workload: String::new(),
                    remote_ip: None,
                }))),
                RawQuery(None),
                headers,
                Bytes::new(),
            )
            .await;
            assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
        }
        // Audit INSERTs use this same connection: zero opens implies zero audit writes.
        assert_eq!(
            state
                .inner
                .lifecycle
                .as_ref()
                .unwrap()
                .control
                .connections
                .load(Ordering::SeqCst),
            0
        );
    }
    #[test]
    fn health_query_excludes_dead_credentials() {
        assert!(pg_query::parse(HEALTH_EXPIRY_SQL).is_ok());
        assert!(HEALTH_EXPIRY_SQL.contains("t.absolute_deadline_ms > $3"));
        assert!(HEALTH_EXPIRY_SQL.contains("t.idle_deadline_ms > $3"));
        assert!(HEALTH_EXPIRY_SQL.contains("x.overlap_until_ms > $3"));
        assert!(HEALTH_EXPIRY_SQL.contains("t.issued_at_ms <= $3"));
    }
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
