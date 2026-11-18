//! Cloud proxy mode, engine side (10 §11.3, B-S12-01): the second kind of token, `oodc_`.
//!
//! puppyone-cloud forwards the control API to this engine. It does not hold the local control token and the
//! local entry never accepts a cloud token. A cloud token is bound to a tenant and a set of Drives, is stored as a
//! BLAKE3 hash (like `oodr_`), expires and can be revoked, and is honoured only on the cloud entry
//! (`api::cloud_router`), a second listener with its own `Host` list. The loopback rules of the local entry do not
//! change (`Origin` refused, `Host` allow list, loopback bind), the cloud entry is not a relaxed copy of it:
//! it has an allow list of routes of its own (`api::cloud_allowed`) and checks the Drive of every call against
//! the token's Drive set.

use serde_json::json;
use sqlx::PgConnection;

use crate::api::error::{ApiError, code};

pub const PREFIX: &str = "oodc_";

/// A verified cloud token.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CloudPrincipal {
    pub id: String,
    pub tenant: String,
    pub drive_ids: Vec<String>,
}

impl CloudPrincipal {
    pub fn allows_drive(&self, drive_id: &str) -> bool {
        self.drive_ids
            .iter()
            .any(|d| d.eq_ignore_ascii_case(drive_id))
    }

    /// The `actor` text of the audit log.
    pub fn actor(&self) -> String {
        format!("cloud:{}", self.id)
    }
}

pub(crate) fn hash(token: &str) -> String {
    blake3::hash(token.as_bytes()).to_hex().to_string()
}

/// Issue a token (shown once to the caller) for `tenant` and `drive_ids`.
pub async fn issue(
    conn: &mut PgConnection,
    tenant: &str,
    drive_ids: &[String],
    label: Option<&str>,
    ttl_seconds: i64,
) -> Result<(String, String), ApiError> {
    let token = crate::share::lifecycle::random_token(PREFIX)?;
    let id: String = sqlx::query_scalar(
        "INSERT INTO _oo_docs.cloud_token (id, token_b3, tenant, drive_ids, label, expires_at) \
         VALUES (gen_random_uuid(), $1, $2, $3::text[]::uuid[], $4, clock_timestamp() + make_interval(secs => $5)) \
         RETURNING id::text",
    )
    .bind(hash(&token))
    .bind(tenant)
    .bind(drive_ids)
    .bind(label)
    .bind(ttl_seconds as f64)
    .fetch_one(&mut *conn)
    .await?;
    Ok((token, id))
}

fn invalid(reason: &str, status: u16) -> ApiError {
    let mut e = ApiError::new(code::UNAUTHENTICATED, "a valid cloud token is required")
        .details(json!({"reason": reason}));
    e.status = status;
    e
}

/// Verify `token`; unknown is 401, revoked or expired is 401 as well (a cloud caller has nothing to
/// distinguish them for, the reason is in the details).
pub async fn authenticate(
    conn: &mut PgConnection,
    token: &str,
) -> Result<CloudPrincipal, ApiError> {
    if !token.starts_with(PREFIX) {
        return Err(invalid("unknown", 401));
    }
    let row: Option<(String, String, Vec<String>, bool, bool)> = sqlx::query_as(
        "SELECT id::text, tenant, drive_ids::text[], revoked_at IS NOT NULL, expires_at <= clock_timestamp() \
           FROM _oo_docs.cloud_token WHERE token_b3 = $1 AND kind = 'cloud'",
    )
    .bind(hash(token))
    .fetch_optional(&mut *conn)
    .await?;
    let Some((id, tenant, drive_ids, revoked, expired)) = row else {
        return Err(invalid("unknown", 401));
    };
    if revoked {
        return Err(invalid("revoked", 401));
    }
    if expired {
        return Err(invalid("expired", 401));
    }
    sqlx::query(
        "UPDATE _oo_docs.cloud_token SET last_used_at = clock_timestamp(), use_count = use_count + 1 WHERE id = $1::uuid",
    )
    .bind(&id)
    .execute(&mut *conn)
    .await?;
    Ok(CloudPrincipal {
        id,
        tenant,
        drive_ids,
    })
}

/// Revoke; true when a live token was revoked.
pub async fn revoke(conn: &mut PgConnection, id: &str) -> Result<bool, sqlx::Error> {
    let done = sqlx::query(
        "UPDATE _oo_docs.cloud_token SET revoked_at = clock_timestamp() WHERE id = $1::uuid AND kind = 'cloud' AND revoked_at IS NULL",
    )
    .bind(id)
    .execute(&mut *conn)
    .await?;
    Ok(done.rows_affected() == 1)
}
