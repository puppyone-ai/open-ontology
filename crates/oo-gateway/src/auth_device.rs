//! Short-lived device grants. Restart invalidates pending grants, never tokens.
use super::*;
use sha2::{Digest, Sha256};

const CLIENT: &str = "oo-cli";
const TTL: Duration = Duration::from_secs(600);
const INTERVAL: Duration = Duration::from_secs(5);
const GRANT_TYPE: &str = "urn:ietf:params:oauth:grant-type:device_code";

struct Pending {
    user_code: String,
    expires: Instant,
    next_poll: Instant,
    interval: Duration,
    approving: bool,
    enrollment: Option<Zeroizing<String>>,
}

#[derive(Default)]
pub(super) struct Devices(HashMap<String, Pending>);

fn hash(code: &str) -> String {
    format!("{:x}", Sha256::digest(code.as_bytes()))
}

fn response(status: u16, value: Value) -> Response {
    let mut response = json_value(status, value);
    response
        .headers_mut()
        .insert(header::CACHE_CONTROL, HeaderValue::from_static("no-store"));
    response
        .headers_mut()
        .insert(header::PRAGMA, HeaderValue::from_static("no-cache"));
    response
}

fn error(code: &str) -> Response {
    response(400, serde_json::json!({"error":code}))
}

// Only the fixed RFC fields are accepted; duplicates cannot override a client ID.
fn form(headers: &HeaderMap, body: &[u8]) -> Result<HashMap<String, String>, ()> {
    if headers
        .get(header::CONTENT_TYPE)
        .and_then(|h| h.to_str().ok())
        .and_then(|s| s.split(';').next())
        != Some("application/x-www-form-urlencoded")
        || body.len() > 1024
    {
        return Err(());
    }
    fn decode(text: &str) -> Result<String, ()> {
        let mut bytes = Vec::new();
        let mut input = text.bytes();
        while let Some(b) = input.next() {
            bytes.push(match b {
                b'+' => b' ',
                b'%' => {
                    let a = char::from(input.next().ok_or(())?).to_digit(16).ok_or(())?;
                    let b = char::from(input.next().ok_or(())?).to_digit(16).ok_or(())?;
                    (a * 16 + b) as u8
                }
                b => b,
            });
        }
        String::from_utf8(bytes).map_err(|_| ())
    }
    let mut fields = HashMap::new();
    for pair in std::str::from_utf8(body).map_err(|_| ())?.split('&') {
        let (key, value) = pair.split_once('=').ok_or(())?;
        let key = decode(key)?;
        if fields.insert(key, decode(value)?).is_some() {
            return Err(());
        }
    }
    if fields.get("client_id").map(String::as_str) != Some(CLIENT) {
        return Err(());
    }
    Ok(fields)
}

pub(super) async fn verification() -> &'static str {
    "Open Ontology device approval: ask your administrator to run `oo token approve <user_code> --grant-file <reviewed-grant.json>` on their authenticated CLI. Confirm the requesting device and role before approving. Never approve a code sent by an unknown party."
}

pub(super) async fn authorize(
    State(state): State<AppState>,
    peer: PublicPeer,
    RawQuery(query): RawQuery,
    headers: HeaderMap,
    body: Bytes,
) -> Response {
    if !tls_gate(&state, &peer, &query, &headers) {
        return error("invalid_request");
    }
    let Some(lifecycle) = &state.inner.lifecycle else {
        return error("access_denied");
    };
    if !lifecycle.admit("device_code", &peer) {
        return response(429, serde_json::json!({"error":"slow_down"}));
    }
    if !form(&headers, &body).is_ok_and(|f| f.len() == 1) {
        return error("invalid_request");
    }
    let mut random = [0u8; 32];
    if getrandom::getrandom(&mut random).is_err() {
        return error("server_error");
    }
    let device_code = Zeroizing::new(
        random
            .iter()
            .map(|b| format!("{b:02x}"))
            .collect::<String>(),
    );
    random.zeroize();
    if getrandom::getrandom(&mut random).is_err() {
        return error("server_error");
    }
    let user_code = format!(
        "{:02X}{:02X}-{:02X}{:02X}",
        random[0], random[1], random[2], random[3]
    );
    random.zeroize();
    let now = Instant::now();
    let digest = hash(&device_code);
    {
        let Ok(mut devices) = lifecycle.devices.lock() else {
            return error("server_error");
        };
        devices.0.retain(|_, pending| pending.expires > now);
        if devices.0.len() >= 256 || devices.0.values().any(|p| p.user_code == user_code) {
            return error("temporarily_unavailable");
        }
        devices.0.insert(
            digest.clone(),
            Pending {
                user_code: user_code.clone(),
                expires: now + TTL,
                next_poll: now + INTERVAL,
                interval: INTERVAL,
                approving: false,
                enrollment: None,
            },
        );
    }
    if lifecycle
        .control
        .audit_device(&audit_actor("device-code", &peer), &digest, false)
        .await
        .is_err()
    {
        if let Ok(mut devices) = lifecycle.devices.lock() {
            devices.0.remove(&digest);
        }
        return error("server_error");
    }
    // The audience may include /v1, but HTTP routes are rooted at the origin.
    let Ok(uri) = state.inner.audience.parse::<axum::http::Uri>() else {
        return error("server_error");
    };
    let Some(authority) = uri.authority() else {
        return error("server_error");
    };
    response(
        200,
        serde_json::json!({
            "device_code": device_code.as_str(), "user_code": user_code,
            "verification_uri": format!("https://{authority}/v1/auth/device"),
            "expires_in": TTL.as_secs(), "interval": INTERVAL.as_secs()
        }),
    )
}

impl Pending {
    fn poll(&mut self, now: Instant) -> Result<Zeroizing<String>, &'static str> {
        if now >= self.expires {
            return Err("expired_token");
        }
        if now < self.next_poll {
            self.interval += INTERVAL;
            self.next_poll = now + self.interval;
            return Err("slow_down");
        }
        self.next_poll = now + self.interval;
        self.enrollment.take().ok_or("authorization_pending")
    }
}

pub(super) async fn poll(
    State(state): State<AppState>,
    peer: PublicPeer,
    RawQuery(query): RawQuery,
    headers: HeaderMap,
    body: Bytes,
) -> Response {
    if !tls_gate(&state, &peer, &query, &headers) {
        return error("invalid_request");
    }
    let Some(lifecycle) = &state.inner.lifecycle else {
        return error("access_denied");
    };
    if !lifecycle.admit("device_poll", &peer) {
        return response(429, serde_json::json!({"error":"slow_down"}));
    }
    let Ok(fields) = form(&headers, &body) else {
        return error("invalid_request");
    };
    if fields.len() != 3 || fields.get("grant_type").map(String::as_str) != Some(GRANT_TYPE) {
        return error("invalid_request");
    }
    let Some(code) = fields
        .get("device_code")
        .filter(|v| v.len() == 64 && v.bytes().all(|b| b.is_ascii_hexdigit()))
    else {
        return error("invalid_grant");
    };
    let digest = hash(code);
    {
        let Ok(devices) = lifecycle.devices.lock() else {
            return error("server_error");
        };
        if !devices.0.contains_key(&digest) {
            // Count every unknown grant cheaply; sample logs without code/peer material.
            let count = lifecycle
                .unknown_device_polls
                .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
            if count % 128 == 0 {
                eprintln!("device_poll_unknown: count={}", count.saturating_add(1));
            }
            return error("expired_token");
        }
    }
    if !lifecycle
        .attempts
        .lock()
        .is_ok_and(|mut attempts| attempts.admit("device_grant", digest.clone()))
    {
        return response(429, serde_json::json!({"error":"slow_down"}));
    }
    // Audit before consuming the single-use grant; an audit outage leaves it retryable.
    if lifecycle
        .control
        .audit_device(&audit_actor("device-poll", &peer), &digest, true)
        .await
        .is_err()
    {
        return error("server_error");
    }
    let enrollment = {
        let Ok(mut devices) = lifecycle.devices.lock() else {
            return error("server_error");
        };
        let Some(pending) = devices.0.get_mut(&digest) else {
            return error("expired_token");
        };
        match pending.poll(Instant::now()) {
            Ok(code) => {
                devices.0.remove(&digest);
                code
            }
            Err(reason) => return error(reason),
        }
    };
    match lifecycle
        .control
        .execute(
            Operation::Redeem {
                code: enrollment,
                audience: state.inner.audience.clone(),
            },
            &format!(
                "{};device_sha256={digest}",
                audit_actor("device-enrollment", &peer)
            ),
        )
        .await
    {
        Ok(mut receipt) => {
            let Some(token) = receipt.secret.take() else {
                return error("server_error");
            };
            let now = token_lifecycle::now_ms().unwrap_or(i64::MAX);
            let remaining = receipt.metadata["expires_at_ms"]
                .as_i64()
                .unwrap_or(now)
                .saturating_sub(now);
            response(
                200,
                serde_json::json!({
                    "access_token": token.as_str(), "token_type":"Bearer",
                    "expires_in": remaining.max(0) / 1000
                }),
            )
        }
        Err(_) => error("access_denied"),
    }
}

pub(super) async fn approve(state: &AppState, body: &[u8], actor: &str) -> Response {
    #[derive(serde::Deserialize)]
    #[serde(deny_unknown_fields)]
    struct Input {
        operation: String,
        user_code: String,
        grant: token_lifecycle::Grant,
    }
    let Ok(input) = serde_json::from_slice::<Input>(body) else {
        return failure("input");
    };
    if input.operation != "device_approve" || input.grant.audience_url != state.inner.audience {
        return failure("audience");
    }
    let Some(lifecycle) = &state.inner.lifecycle else {
        return failure("disabled");
    };
    let digest = {
        let Ok(mut devices) = lifecycle.devices.lock() else {
            return failure("database");
        };
        let Some((digest, pending)) = devices
            .0
            .iter_mut()
            .find(|(_, p)| p.user_code == input.user_code)
        else {
            return failure("not_found");
        };
        if pending.expires <= Instant::now() || pending.approving || pending.enrollment.is_some() {
            return failure("expired_or_already_approved");
        }
        // Never hold a synchronous mutex across database IO.
        pending.approving = true;
        digest.clone()
    };
    let result = lifecycle
        .control
        .execute(Operation::Enroll(input.grant), actor)
        .await;
    let Ok(mut devices) = lifecycle.devices.lock() else {
        return failure("database");
    };
    let Some(pending) = devices.0.get_mut(&digest) else {
        return failure("expired");
    };
    match result {
        Ok(mut receipt) if pending.expires > Instant::now() => {
            pending.enrollment = receipt.secret.take();
            response(200, serde_json::json!({"approved":true}))
        }
        Ok(_) => failure("expired"),
        Err(reason) => {
            pending.approving = false;
            failure(reason)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn polling_slow_down_is_cumulative_and_expiry_wins() {
        let now = Instant::now();
        let mut pending = Pending {
            user_code: String::new(),
            expires: now + TTL,
            next_poll: now + INTERVAL,
            interval: INTERVAL,
            approving: false,
            enrollment: None,
        };
        assert_eq!(pending.poll(now).unwrap_err(), "slow_down");
        assert_eq!(pending.interval.as_secs(), 10);
        assert_eq!(pending.poll(now).unwrap_err(), "slow_down");
        assert_eq!(pending.interval.as_secs(), 15);
        assert_eq!(
            pending.poll(now + Duration::from_secs(15)).unwrap_err(),
            "authorization_pending"
        );
        pending.enrollment = Some(Zeroizing::new(String::new()));
        assert!(pending.poll(now + Duration::from_secs(30)).is_ok());
        assert_eq!(pending.poll(now + TTL).unwrap_err(), "expired_token");
    }
    #[test]
    fn form_rejects_duplicate_client_and_wrong_content_type() {
        let mut headers = HeaderMap::new();
        assert!(form(&headers, b"client_id=oo-cli").is_err());
        headers.insert(
            header::CONTENT_TYPE,
            HeaderValue::from_static("application/x-www-form-urlencoded"),
        );
        assert!(form(&headers, b"client_id=oo-cli&client_id=other").is_err());
        assert_eq!(
            form(&headers, b"client_id=oo-cli&grant_type=a%3Ab").unwrap()["grant_type"],
            "a:b"
        );
        assert!(form(&headers, b"client_id=oo-cli&x=%GG").is_err());
    }
}
