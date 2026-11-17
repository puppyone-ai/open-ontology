//! refs credentials (10 §7.8): `oodr_` + base64url(32 random bytes), stored as
//! a BLAKE3 hash, bound to one share, `refs:read` only, dead on expiry,
//! revocation and refresh. 60 reads per minute per credential.
//!
//! A credential may also authenticate the share's own task-time calls
//! (`:check`, `updates`, entity reads with `as_of=share`), never anything else.

use std::collections::{HashMap, VecDeque};
use std::sync::{Mutex, OnceLock};
use std::time::{Duration, Instant};

use serde_json::json;
use sqlx::PgConnection;

use crate::api::error::{ApiError, code};

/// Reads per credential per minute.
pub const RATE_PER_MINUTE: usize = 60;

/// A verified credential and the share behind it.
#[derive(Clone, Debug)]
pub struct ShareCred {
    pub cred_id: String,
    pub share_id: String,
    pub drive_id: String,
    pub scopes: Vec<String>,
    pub version: i64,
    pub scope: Vec<String>,
}

fn ended(reason: &str) -> ApiError {
    let mut e = ApiError::new(
        code::SHARE_REFS_CREDENTIAL_INVALID,
        "the credential is no longer valid",
    )
    .details(json!({"reason": reason}));
    e.status = 410;
    e
}

/// Verify `token` (an `oodr_...` string). Unknown: 401. Expired, revoked or
/// the share ended: 410.
pub async fn authenticate(conn: &mut PgConnection, token: &str) -> Result<ShareCred, ApiError> {
    let hash = blake3::hash(token.as_bytes()).to_hex().to_string();
    #[allow(clippy::type_complexity)]
    let row: Option<(String, String, String, Vec<String>, bool, bool, String, i64, String)> =
        sqlx::query_as(
            "SELECT c.id::text, c.share_id::text, s.drive_id::text, c.scopes, \
                    c.revoked_at IS NOT NULL, (c.expires_at <= clock_timestamp() OR s.expires_at <= clock_timestamp()), s.status, \
                    p.version_n, s.scope::text \
               FROM _oo_docs.refs_credential c JOIN _oo_docs.share s ON s.id = c.share_id \
               JOIN _oo_docs.context_pack p ON p.id = s.pack_id WHERE c.token_b3 = $1",
        )
        .bind(&hash)
        .fetch_optional(&mut *conn)
        .await?;
    let Some((cred_id, share_id, drive_id, scopes, revoked, expired, status, version, scope)) = row
    else {
        return Err(ApiError::new(
            code::SHARE_REFS_CREDENTIAL_INVALID,
            "unknown credential",
        ));
    };
    if revoked {
        return Err(ended("revoked"));
    }
    if expired || status == "expired" {
        return Err(ended("expired"));
    }
    if !matches!(status.as_str(), "active" | "drifted") {
        return Err(ended(&status));
    }
    Ok(ShareCred {
        cred_id,
        share_id,
        drive_id,
        scopes,
        version,
        scope: serde_json::from_str(&scope).unwrap_or_default(),
    })
}

fn limiter() -> &'static Mutex<HashMap<String, VecDeque<Instant>>> {
    static L: OnceLock<Mutex<HashMap<String, VecDeque<Instant>>>> = OnceLock::new();
    L.get_or_init(|| Mutex::new(HashMap::new()))
}

/// Count one read; `Err(retry_after_ms)` past the limit.
pub fn take(cred_id: &str, per_minute: usize) -> Result<(), u64> {
    let mut map = limiter().lock().unwrap_or_else(|p| p.into_inner());
    let q = map.entry(cred_id.to_owned()).or_default();
    let now = Instant::now();
    while q
        .front()
        .is_some_and(|t| now.duration_since(*t) >= Duration::from_secs(60))
    {
        q.pop_front();
    }
    if q.len() >= per_minute {
        let wait = Duration::from_secs(60).saturating_sub(now.duration_since(q[0]));
        return Err(wait.as_millis() as u64 + 1);
    }
    q.push_back(now);
    Ok(())
}

/// Record a use.
pub async fn touch(conn: &mut PgConnection, cred_id: &str) -> Result<(), ApiError> {
    sqlx::query(
        "UPDATE _oo_docs.refs_credential SET last_used_at = clock_timestamp(), use_count = use_count + 1 \
          WHERE id = $1::uuid",
    )
    .bind(cred_id)
    .execute(&mut *conn)
    .await?;
    Ok(())
}

/// The original of `b3`, only when the share's snapshot holds a live file
/// version with that hash that carries evidence (what the pack cites) and its
/// path is inside the share's scope. Returns `(drive_id, raw_key, size)`.
pub async fn locate(
    conn: &mut PgConnection,
    cred: &ShareCred,
    b3: &str,
) -> Result<(String, Vec<u8>, i64), ApiError> {
    let denied = || {
        ApiError::new(
            code::SHARE_REFS_OUT_OF_SCOPE,
            "the file is outside this share",
        )
    };
    if b3.len() != 64 || !b3.bytes().all(|c| c.is_ascii_hexdigit()) {
        return Err(denied());
    }
    let row: Option<(Vec<u8>, i64)> = sqlx::query_as(
        "SELECT f.raw_key, b.size FROM _oo_docs.file_version fv \
           JOIN _oo_docs.file f ON f.id = fv.file_id JOIN _oo_docs.blob b ON b.b3 = fv.b3 \
           JOIN _oo_docs.share s ON s.id = $1::uuid \
           JOIN _oo_docs.context_pack p ON p.id = s.pack_id \
           JOIN _oo_docs.ontology_version ov ON ov.n = p.version_n \
          WHERE fv.b3 = $2 AND f.drive_id = s.drive_id \
            AND fv.created_at <= ov.published_at \
            AND (fv.superseded_at IS NULL OR fv.superseded_at > ov.published_at) \
            AND (fv.deleted_at IS NULL OR fv.deleted_at > ov.published_at) \
            AND (jsonb_array_length(s.scope) = 0 OR EXISTS (SELECT 1 FROM jsonb_array_elements_text(s.scope) sp(p) \
                 WHERE fv.path_at_version = sp.p OR fv.path_at_version LIKE \
                       replace(replace(replace(sp.p, '\\', '\\\\'), '%', '\\%'), '_', '\\_') || '/%')) \
            AND EXISTS (SELECT 1 FROM _oo_docs.evidence e WHERE e.version_id = fv.id) \
          LIMIT 1",
    )
    .bind(&cred.share_id)
    .bind(b3)
    .fetch_optional(&mut *conn)
    .await?;
    let (raw_key, size) = row.ok_or_else(denied)?;
    Ok((cred.drive_id.clone(), raw_key, size))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_limit_is_per_credential_and_per_minute() {
        for _ in 0..RATE_PER_MINUTE {
            assert!(take("test-limit-a", RATE_PER_MINUTE).is_ok());
        }
        assert!(take("test-limit-a", RATE_PER_MINUTE).is_err());
        assert!(take("test-limit-b", RATE_PER_MINUTE).is_ok());
    }
}
