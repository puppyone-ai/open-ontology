//! Idempotency store (10 §8.1): `(route, key)` keeps the body hash and the
//! complete response for 24 hours. Same key and body replays the stored
//! response; same key with another body is a 409.

use axum::http::{HeaderMap, HeaderName};
use serde_json::Value;
use sha2::{Digest, Sha256};
use sqlx::PgConnection;

use super::error::{ApiError, code};

/// Header carrying the key.
pub const HEADER: HeaderName = HeaderName::from_static("idempotency-key");
/// Retention of a stored response.
pub const RETENTION_HOURS: i32 = 24;

/// `^[A-Za-z0-9_-]{16,128}$`.
pub fn valid_key(key: &str) -> bool {
    (16..=128).contains(&key.len())
        && key
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'_' || b == b'-')
}

/// The required key of a side-effecting POST.
pub fn key_from(headers: &HeaderMap) -> Result<String, ApiError> {
    let raw = headers.get(HEADER).ok_or_else(|| {
        ApiError::new(
            code::IDEMPOTENCY_KEY_REQUIRED,
            "Idempotency-Key is required",
        )
    })?;
    let key = raw
        .to_str()
        .map_err(|_| ApiError::bad("/headers/Idempotency-Key", "not text"))?;
    if !valid_key(key) {
        return Err(ApiError::bad(
            "/headers/Idempotency-Key",
            "expected 16 to 128 characters of [A-Za-z0-9_-]",
        ));
    }
    Ok(key.to_owned())
}

/// SHA-256 hex of the raw body.
pub fn body_sha256(body: &[u8]) -> String {
    Sha256::digest(body)
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect()
}

/// A stored response.
#[derive(Clone, Debug)]
pub struct Stored {
    /// HTTP status.
    pub status: u16,
    /// Response body.
    pub response: Value,
}

/// Look the key up. `Ok(None)` when absent or older than 24 h (the stale row
/// is dropped). A different body hash is [`code::IDEMPOTENCY_CONFLICT`].
pub async fn lookup(
    conn: &mut PgConnection,
    route: &str,
    key: &str,
    body_sha: &str,
) -> Result<Option<Stored>, ApiError> {
    sqlx::query(
        "DELETE FROM _oo_docs.api_idempotency WHERE route = $1 AND key = $2 \
         AND created_at < clock_timestamp() - make_interval(hours => $3)",
    )
    .bind(route)
    .bind(key)
    .bind(RETENTION_HOURS)
    .execute(&mut *conn)
    .await?;
    let row: Option<(String, i16, String)> = sqlx::query_as(
        "SELECT body_sha256, status, response::text FROM _oo_docs.api_idempotency \
         WHERE route = $1 AND key = $2",
    )
    .bind(route)
    .bind(key)
    .fetch_optional(&mut *conn)
    .await?;
    let Some((stored_sha, status, response)) = row else {
        return Ok(None);
    };
    if stored_sha != body_sha {
        return Err(ApiError::new(
            code::IDEMPOTENCY_CONFLICT,
            "the key was used with a different body",
        ));
    }
    Ok(Some(Stored {
        status: status as u16,
        response: serde_json::from_str(&response).unwrap_or(Value::Null),
    }))
}

/// Store a response; a concurrent first writer wins.
pub async fn store(
    conn: &mut PgConnection,
    route: &str,
    key: &str,
    body_sha: &str,
    status: u16,
    response: &Value,
) -> Result<(), ApiError> {
    sqlx::query(
        "INSERT INTO _oo_docs.api_idempotency (key, route, body_sha256, status, response) \
         VALUES ($1, $2, $3, $4, $5::jsonb) ON CONFLICT (route, key) DO NOTHING",
    )
    .bind(key)
    .bind(route)
    .bind(body_sha)
    .bind(status as i16)
    .bind(response.to_string())
    .execute(&mut *conn)
    .await?;
    Ok(())
}

/// Drop expired rows (engine housekeeping).
pub async fn purge_expired(conn: &mut PgConnection) -> Result<u64, ApiError> {
    Ok(sqlx::query(
        "DELETE FROM _oo_docs.api_idempotency \
         WHERE created_at < clock_timestamp() - make_interval(hours => $1)",
    )
    .bind(RETENTION_HOURS)
    .execute(&mut *conn)
    .await?
    .rows_affected())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn key_shape() {
        assert!(valid_key("abcdefgh12345678"));
        assert!(valid_key(&"a".repeat(128)));
        assert!(!valid_key("short"));
        assert!(!valid_key(&"a".repeat(129)));
        assert!(!valid_key("abcdefgh1234567!"));
    }

    #[test]
    fn missing_and_bad_keys() {
        let mut h = HeaderMap::new();
        assert_eq!(
            key_from(&h).unwrap_err().code,
            code::IDEMPOTENCY_KEY_REQUIRED
        );
        h.insert(HEADER, "x".parse().unwrap());
        assert_eq!(key_from(&h).unwrap_err().code, code::BAD_REQUEST);
        h.insert(HEADER, "abcdefgh12345678".parse().unwrap());
        assert_eq!(key_from(&h).unwrap(), "abcdefgh12345678");
    }

    #[test]
    fn body_hash_is_sha256() {
        assert_eq!(
            body_sha256(b""),
            "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855"
        );
    }
}
