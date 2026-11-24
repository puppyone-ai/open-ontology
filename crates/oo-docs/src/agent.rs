//! Agent tokens, engine side (D83): the third kind of token, `ooa_`.
//!
//! One token, handed to any Agent, scoped, with a TTL, revocable and audited. It is built on the `oodc_` cloud
//! token code ([`crate::cloud`]) and shares its table (`_oo_docs.cloud_token`, `kind = 'agent'`), its hash
//! (BLAKE3, only the hash is stored), its random source and its revoke. What differs:
//!
//! * Scopes: `docs:read` (the document ontology: entities, resolve, evidence, files, links), `db:read` (the
//!   database ontology; the engine does not serve it, the CLI proxy reads it through the Gateway with the
//!   Gateway profile named in the token, so the Agent never holds a Gateway token), and the optional
//!   `docs:suggest` (post a Suggest) and `docs:write` (decide a link).
//! * Drives: an explicit set, or `all_drives`. A Drive outside the set is as good as absent.
//! * It is honoured on the local entry only (loopback, the same Host and Origin guards as the control token);
//!   the cloud entry never takes it and a cloud token is never an Agent token.
//! * The engine checks the scope on every route (`api::agent_guard`), a call is one audit row, a refusal too.

use serde_json::{Value, json};
use sqlx::PgConnection;

use crate::api::error::{ApiError, code};

pub const PREFIX: &str = "ooa_";

pub const DOCS_READ: &str = "docs:read";
pub const DOCS_SUGGEST: &str = "docs:suggest";
pub const DOCS_WRITE: &str = "docs:write";
pub const DB_READ: &str = "db:read";
/// Every scope there is, in the order they are listed.
pub const SCOPES: [&str; 4] = [DOCS_READ, DOCS_SUGGEST, DOCS_WRITE, DB_READ];

/// Seven days, like the cloud token.
pub const DEFAULT_TTL: i64 = 7 * 86_400;
pub const MIN_TTL: i64 = 60;
pub const MAX_TTL: i64 = 2_592_000;

/// A verified Agent token.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AgentPrincipal {
    pub id: String,
    pub name: String,
    pub scopes: Vec<String>,
    pub drive_ids: Vec<String>,
    pub all_drives: bool,
    pub gw_profile: Option<String>,
    pub expires_at: String,
}

impl AgentPrincipal {
    pub fn has_scope(&self, scope: &str) -> bool {
        self.scopes.iter().any(|s| s == scope)
    }

    pub fn allows_drive(&self, drive_id: &str) -> bool {
        self.all_drives
            || self
                .drive_ids
                .iter()
                .any(|d| d.eq_ignore_ascii_case(drive_id))
    }

    /// The `actor` text of the audit log.
    pub fn actor(&self) -> String {
        format!("agent:{}", self.name)
    }

    /// What `GET /v1/docs/agent-tokens/self` and the introspection answer (never the token).
    pub fn to_json(&self) -> Value {
        json!({
            "id": self.id, "name": self.name, "scopes": self.scopes, "drive_ids": self.drive_ids,
            "all_drives": self.all_drives, "gw_profile": self.gw_profile, "expires_at": self.expires_at,
        })
    }
}

/// Scope names a request may carry, validated and sorted.
pub fn parse_scopes(value: &Value) -> Result<Vec<String>, ApiError> {
    let bad = |m: &str| ApiError::bad("/scopes", m);
    let arr = value
        .as_array()
        .filter(|a| (1..=SCOPES.len()).contains(&a.len()))
        .ok_or_else(|| bad("1 to 4 scopes"))?;
    let mut out: Vec<String> = Vec::new();
    for v in arr {
        let s = v
            .as_str()
            .filter(|s| SCOPES.contains(s))
            .ok_or_else(|| bad("scopes are docs:read, docs:suggest, docs:write, db:read"))?;
        if !out.iter().any(|o| o == s) {
            out.push(s.to_owned());
        }
    }
    // Writing or suggesting on documents needs reading them first.
    if (out.iter().any(|s| s == DOCS_SUGGEST || s == DOCS_WRITE))
        && !out.iter().any(|s| s == DOCS_READ)
    {
        return Err(bad("docs:suggest and docs:write need docs:read"));
    }
    out.sort_by_key(|s| SCOPES.iter().position(|k| k == s));
    Ok(out)
}

/// A name the CLI and the audit trail use: 1..=64 of `[A-Za-z0-9._-]`.
pub fn valid_name(name: &str) -> bool {
    (1..=64).contains(&name.len())
        && name
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'.' | b'_' | b'-'))
}

/// What to issue.
pub struct NewAgent<'a> {
    pub name: &'a str,
    pub scopes: &'a [String],
    pub drive_ids: &'a [String],
    pub all_drives: bool,
    pub gw_profile: Option<&'a str>,
    pub label: Option<&'a str>,
    pub ttl_seconds: i64,
}

/// Issue a token (shown once to the caller). A live token of the same name is a conflict.
pub async fn issue(
    conn: &mut PgConnection,
    n: &NewAgent<'_>,
) -> Result<(String, String), ApiError> {
    let token = crate::share::lifecycle::random_token(PREFIX)?;
    let row: Result<String, sqlx::Error> = sqlx::query_scalar(
        "INSERT INTO _oo_docs.cloud_token \
           (id, token_b3, tenant, drive_ids, label, expires_at, kind, scopes, all_drives, gw_profile, agent_name) \
         VALUES (gen_random_uuid(), $1, 'local', $2::text[]::uuid[], $3, \
                 clock_timestamp() + make_interval(secs => $4), 'agent', $5, $6, $7, $8) \
         RETURNING id::text",
    )
    .bind(crate::cloud::hash(&token))
    .bind(n.drive_ids)
    .bind(n.label)
    .bind(n.ttl_seconds as f64)
    .bind(n.scopes)
    .bind(n.all_drives)
    .bind(n.gw_profile)
    .bind(n.name)
    .fetch_one(&mut *conn)
    .await;
    match row {
        Ok(id) => Ok((token, id)),
        Err(sqlx::Error::Database(e)) if e.is_unique_violation() => Err(ApiError::bad(
            "/name",
            "a live agent token of this name exists; revoke it first",
        )),
        Err(e) => Err(e.into()),
    }
}

fn invalid(reason: &str) -> ApiError {
    let mut e = ApiError::new(code::UNAUTHENTICATED, "a valid agent token is required")
        .details(json!({"reason": reason}));
    e.status = 401;
    e
}

/// Verify `token`: unknown, revoked and expired are all 401 (the reason is in the details).
pub async fn authenticate(
    conn: &mut PgConnection,
    token: &str,
    touch: bool,
) -> Result<AgentPrincipal, ApiError> {
    if !token.starts_with(PREFIX) {
        return Err(invalid("unknown"));
    }
    type Row = (
        String,
        String,
        Vec<String>,
        Vec<String>,
        bool,
        Option<String>,
        String,
        bool,
        bool,
    );
    let row: Option<Row> = sqlx::query_as(
        "SELECT id::text, COALESCE(agent_name, ''), scopes, drive_ids::text[], all_drives, gw_profile, \
                to_char(expires_at AT TIME ZONE 'UTC', 'YYYY-MM-DD\"T\"HH24:MI:SS\"Z\"'), \
                revoked_at IS NOT NULL, expires_at <= clock_timestamp() \
           FROM _oo_docs.cloud_token WHERE token_b3 = $1 AND kind = 'agent'",
    )
    .bind(crate::cloud::hash(token))
    .fetch_optional(&mut *conn)
    .await?;
    let Some((id, name, scopes, drive_ids, all_drives, gw_profile, expires_at, revoked, expired)) =
        row
    else {
        return Err(invalid("unknown"));
    };
    if revoked {
        return Err(invalid("revoked"));
    }
    if expired {
        return Err(invalid("expired"));
    }
    if touch {
        sqlx::query(
            "UPDATE _oo_docs.cloud_token SET last_used_at = clock_timestamp(), use_count = use_count + 1 WHERE id = $1::uuid",
        )
        .bind(&id)
        .execute(&mut *conn)
        .await?;
    }
    Ok(AgentPrincipal {
        id,
        name,
        scopes,
        drive_ids,
        all_drives,
        gw_profile,
        expires_at,
    })
}

/// Tokens as `oo agent token ls` shows them: no hash, no token.
pub async fn list(conn: &mut PgConnection, include_dead: bool) -> Result<Vec<Value>, sqlx::Error> {
    let rows: Vec<(String,)> = sqlx::query_as(
        "SELECT jsonb_build_object('id', id::text, 'name', agent_name, 'scopes', scopes, \
                'drive_ids', drive_ids::text[], 'all_drives', all_drives, 'gw_profile', gw_profile, \
                'label', label, 'created_at', to_char(created_at AT TIME ZONE 'UTC', 'YYYY-MM-DD\"T\"HH24:MI:SS\"Z\"'), \
                'expires_at', to_char(expires_at AT TIME ZONE 'UTC', 'YYYY-MM-DD\"T\"HH24:MI:SS\"Z\"'), \
                'revoked_at', to_char(revoked_at AT TIME ZONE 'UTC', 'YYYY-MM-DD\"T\"HH24:MI:SS\"Z\"'), \
                'last_used_at', to_char(last_used_at AT TIME ZONE 'UTC', 'YYYY-MM-DD\"T\"HH24:MI:SS\"Z\"'), \
                'use_count', use_count, \
                'state', CASE WHEN revoked_at IS NOT NULL THEN 'revoked' \
                              WHEN expires_at <= clock_timestamp() THEN 'expired' ELSE 'live' END)::text \
           FROM _oo_docs.cloud_token WHERE kind = 'agent' \
            AND ($1 OR (revoked_at IS NULL AND expires_at > clock_timestamp())) \
          ORDER BY created_at DESC, id LIMIT 500",
    )
    .bind(include_dead)
    .fetch_all(&mut *conn)
    .await?;
    Ok(rows
        .into_iter()
        .filter_map(|(j,)| serde_json::from_str(&j).ok())
        .collect())
}

/// Revoke an Agent token by id; true when a live one was revoked.
pub async fn revoke(conn: &mut PgConnection, id: &str) -> Result<bool, sqlx::Error> {
    let done = sqlx::query(
        "UPDATE _oo_docs.cloud_token SET revoked_at = clock_timestamp() \
          WHERE id = $1::uuid AND kind = 'agent' AND revoked_at IS NULL",
    )
    .bind(id)
    .execute(&mut *conn)
    .await?;
    Ok(done.rows_affected() == 1)
}

/// The name of a token (for the audit row of its revocation).
pub async fn name_of(conn: &mut PgConnection, id: &str) -> Result<Option<String>, sqlx::Error> {
    sqlx::query_scalar(
        "SELECT agent_name FROM _oo_docs.cloud_token WHERE id = $1::uuid AND kind = 'agent'",
    )
    .bind(id)
    .fetch_optional(&mut *conn)
    .await
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn scopes_are_closed_sorted_and_write_needs_read() {
        assert_eq!(
            parse_scopes(&json!(["db:read", "docs:read", "docs:read"])).unwrap(),
            ["docs:read", "db:read"]
        );
        assert!(parse_scopes(&json!([])).is_err());
        assert!(parse_scopes(&json!(["docs:admin"])).is_err());
        assert!(parse_scopes(&json!(["docs:write"])).is_err());
        assert!(parse_scopes(&json!(["docs:read", "docs:write", "docs:suggest"])).is_ok());
        assert!(parse_scopes(&json!("docs:read")).is_err());
    }

    #[test]
    fn names_and_drive_scope() {
        assert!(valid_name("sales-bot.1"));
        assert!(!valid_name(""));
        assert!(!valid_name("a b"));
        assert!(!valid_name(&"x".repeat(65)));
        let p = AgentPrincipal {
            id: "i".into(),
            name: "bot".into(),
            scopes: vec![DOCS_READ.into()],
            drive_ids: vec!["0F8FAD5B-D9CB-469F-A165-70867728950E".into()],
            all_drives: false,
            gw_profile: None,
            expires_at: String::new(),
        };
        assert!(p.allows_drive("0f8fad5b-d9cb-469f-a165-70867728950e"));
        assert!(!p.allows_drive("00000000-0000-4000-8000-000000000001"));
        assert!(p.has_scope(DOCS_READ) && !p.has_scope(DB_READ));
        assert_eq!(p.actor(), "agent:bot");
    }
}
