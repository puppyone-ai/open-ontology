//! Append-only audit of sharing (10 §11.3, B-S12-01, OO side): `share.created`, `share.revoked`, `refs.accessed`
//! and the issue and revocation of cloud tokens go into `_oo_docs.audit_log`, so that "who shared what to which
//! agent, and when" has an answer that cannot be edited afterwards.
//!
//! * Insert only: the table rejects UPDATE, DELETE and TRUNCATE (trigger), the runtime role has no such grant.
//! * Hash chained: every row stores the hash of the previous row and its own; [`verify_chain`] recomputes both
//!   from the stored columns, so a deleted or changed row is found. Inserts take one advisory lock, so the chain
//!   has no forks.
//! * No secret, no content: entries name ids, the scope (path prefixes), the role, the grantor and the agent
//!   label. Never a token, a credential, a path under the scope or a block of text.
//!
//! The actor is who made the call: `local` (the control token on the loopback API), `cloud:<token id>` (a cloud
//! proxy token, with its tenant) or `cred:<credential id>` (a share's `oodr_` credential reading a ref).

use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use sqlx::{Connection, PgConnection, Row};

const CHAIN_LOCK: i64 = 6_413_202_612;
/// `prev_hash` of the first row.
pub const GENESIS: &str = "genesis";

pub const SHARE_CREATED: &str = "share.created";
pub const SHARE_REVOKED: &str = "share.revoked";
pub const REFS_ACCESSED: &str = "refs.accessed";
pub const TOKEN_ISSUED: &str = "cloud_token.issued";
pub const TOKEN_REVOKED: &str = "cloud_token.revoked";

/// One entry to append.
#[derive(Clone, Debug)]
pub struct Entry<'a> {
    pub action: &'a str,
    pub actor: &'a str,
    pub tenant: Option<&'a str>,
    pub drive_id: Option<&'a str>,
    pub share_id: Option<&'a str>,
    pub agent: Option<&'a str>,
    pub detail: Value,
}

/// The columns the hash covers, in one place.
struct HashRow<'a> {
    prev: &'a str,
    at: &'a str,
    action: &'a str,
    actor: &'a str,
    tenant: Option<&'a str>,
    drive: Option<&'a str>,
    share: Option<&'a str>,
    agent: Option<&'a str>,
    detail: &'a Value,
}

impl HashRow<'_> {
    fn hash(&self) -> String {
        let text = json!([
            self.prev,
            self.at,
            self.action,
            self.actor,
            self.tenant,
            self.drive,
            self.share,
            self.agent,
            self.detail
        ])
        .to_string();
        format!("{:x}", Sha256::digest(text.as_bytes()))
    }
}

/// Append one entry; returns its sequence number.
pub async fn record(conn: &mut PgConnection, e: &Entry<'_>) -> Result<i64, sqlx::Error> {
    let mut tx = conn.begin().await?;
    sqlx::query("SELECT pg_advisory_xact_lock($1)")
        .bind(CHAIN_LOCK)
        .execute(&mut *tx)
        .await?;
    let prev: Option<String> =
        sqlx::query_scalar("SELECT row_hash FROM _oo_docs.audit_log ORDER BY seq DESC LIMIT 1")
            .fetch_optional(&mut *tx)
            .await?;
    let prev = prev.unwrap_or_else(|| GENESIS.to_owned());
    let at: String = sqlx::query_scalar("SELECT clock_timestamp()::text")
        .fetch_one(&mut *tx)
        .await?;
    let hash = HashRow {
        prev: &prev,
        at: &at,
        action: e.action,
        actor: e.actor,
        tenant: e.tenant,
        drive: e.drive_id,
        share: e.share_id,
        agent: e.agent,
        detail: &e.detail,
    }
    .hash();
    let seq: i64 = sqlx::query_scalar(
        "INSERT INTO _oo_docs.audit_log \
           (at, action, actor, tenant, drive_id, share_id, agent, detail, prev_hash, row_hash) \
         VALUES ($1::timestamptz, $2, $3, $4, $5::uuid, $6::uuid, $7, $8::jsonb, $9, $10) RETURNING seq",
    )
    .bind(&at)
    .bind(e.action)
    .bind(e.actor)
    .bind(e.tenant)
    .bind(e.drive_id)
    .bind(e.share_id)
    .bind(e.agent)
    .bind(e.detail.to_string())
    .bind(&prev)
    .bind(&hash)
    .fetch_one(&mut *tx)
    .await?;
    tx.commit().await?;
    Ok(seq)
}

/// Result of [`verify_chain`].
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Chain {
    Intact {
        rows: i64,
    },
    /// The first row whose link or hash does not match what the rows before it say.
    Broken {
        seq: i64,
    },
}

/// Recompute the chain from the stored columns.
pub async fn verify_chain(conn: &mut PgConnection) -> Result<Chain, sqlx::Error> {
    let rows = sqlx::query(
        "SELECT seq, at::text AS at, action, actor, tenant, drive_id::text AS drive, share_id::text AS share, \
                agent, detail::text AS detail, prev_hash, row_hash \
           FROM _oo_docs.audit_log ORDER BY seq",
    )
    .fetch_all(&mut *conn)
    .await?;
    let mut prev = GENESIS.to_owned();
    for r in &rows {
        let seq: i64 = r.try_get("seq")?;
        let detail: Value =
            serde_json::from_str(&r.try_get::<String, _>("detail")?).unwrap_or(Value::Null);
        let at: String = r.try_get("at")?;
        let (action, actor): (String, String) = (r.try_get("action")?, r.try_get("actor")?);
        let (tenant, drive, share, agent): (
            Option<String>,
            Option<String>,
            Option<String>,
            Option<String>,
        ) = (
            r.try_get("tenant")?,
            r.try_get("drive")?,
            r.try_get("share")?,
            r.try_get("agent")?,
        );
        let want = HashRow {
            prev: &prev,
            at: &at,
            action: &action,
            actor: &actor,
            tenant: tenant.as_deref(),
            drive: drive.as_deref(),
            share: share.as_deref(),
            agent: agent.as_deref(),
            detail: &detail,
        }
        .hash();
        if r.try_get::<String, _>("prev_hash")? != prev
            || r.try_get::<String, _>("row_hash")? != want
        {
            return Ok(Chain::Broken { seq });
        }
        prev = want;
    }
    Ok(Chain::Intact {
        rows: rows.len() as i64,
    })
}

/// A filter of [`who_shared`].
#[derive(Clone, Debug, Default)]
pub struct SharedFilter<'a> {
    pub drive_id: Option<&'a str>,
    pub agent: Option<&'a str>,
    pub tenant: Option<&'a str>,
    /// Inclusive lower bound, a timestamp text.
    pub since: Option<&'a str>,
    pub until: Option<&'a str>,
}

/// One answer: who shared what to which agent, when.
#[derive(Clone, Debug, PartialEq)]
pub struct Shared {
    pub seq: i64,
    pub at: String,
    pub actor: String,
    pub tenant: Option<String>,
    pub drive_id: Option<String>,
    pub share_id: String,
    pub agent: Option<String>,
    pub grantor: Option<String>,
    pub role: Option<String>,
    pub scope: Value,
    pub revoked_at: Option<String>,
    pub refs_reads: i64,
}

/// Every share created that matches `filter`, oldest first.
pub async fn who_shared(
    conn: &mut PgConnection,
    f: &SharedFilter<'_>,
) -> Result<Vec<Shared>, sqlx::Error> {
    let rows = sqlx::query(
        "SELECT c.seq, c.at::text AS at, c.actor, c.tenant, c.drive_id::text AS drive, c.share_id::text AS share, \
                c.agent, c.detail::text AS detail, \
                (SELECT min(r.at)::text FROM _oo_docs.audit_log r \
                  WHERE r.action = 'share.revoked' AND r.share_id = c.share_id) AS revoked_at, \
                (SELECT count(*) FROM _oo_docs.audit_log a \
                  WHERE a.action = 'refs.accessed' AND a.share_id = c.share_id) AS reads \
           FROM _oo_docs.audit_log c \
          WHERE c.action = 'share.created' \
            AND ($1::uuid IS NULL OR c.drive_id = $1::uuid) \
            AND ($2::text IS NULL OR c.agent = $2) \
            AND ($3::text IS NULL OR c.tenant = $3) \
            AND ($4::timestamptz IS NULL OR c.at >= $4::timestamptz) \
            AND ($5::timestamptz IS NULL OR c.at < $5::timestamptz) \
          ORDER BY c.seq",
    )
    .bind(f.drive_id)
    .bind(f.agent)
    .bind(f.tenant)
    .bind(f.since)
    .bind(f.until)
    .fetch_all(&mut *conn)
    .await?;
    let mut out = Vec::with_capacity(rows.len());
    for r in rows {
        let detail: Value =
            serde_json::from_str(&r.try_get::<String, _>("detail")?).unwrap_or(Value::Null);
        out.push(Shared {
            seq: r.try_get("seq")?,
            at: r.try_get("at")?,
            actor: r.try_get("actor")?,
            tenant: r.try_get("tenant")?,
            drive_id: r.try_get("drive")?,
            share_id: r.try_get("share")?,
            agent: r.try_get("agent")?,
            grantor: detail
                .get("grantor")
                .and_then(Value::as_str)
                .map(str::to_owned),
            role: detail
                .get("role")
                .and_then(Value::as_str)
                .map(str::to_owned),
            scope: detail.get("scope").cloned().unwrap_or(Value::Null),
            revoked_at: r.try_get("revoked_at")?,
            refs_reads: r.try_get("reads")?,
        });
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn row<'a>(
        prev: &'a str,
        action: &'a str,
        tenant: Option<&'a str>,
        agent: &'a str,
        d: &'a Value,
    ) -> String {
        HashRow {
            prev,
            at: "t",
            action,
            actor: "local",
            tenant,
            drive: None,
            share: Some("s"),
            agent: Some(agent),
            detail: d,
        }
        .hash()
    }

    #[test]
    fn the_hash_covers_every_column_and_the_link() {
        let d = json!({"role": "viewer"});
        let a = row("p", "share.created", None, "agent", &d);
        assert_eq!(a.len(), 64);
        assert_ne!(a, row("q", "share.created", None, "agent", &d));
        assert_ne!(a, row("p", "share.created", None, "other", &d));
        assert_ne!(a, row("p", "share.revoked", None, "agent", &d));
        assert_ne!(a, row("p", "share.created", Some("x"), "agent", &d));
        assert_ne!(
            a,
            row(
                "p",
                "share.created",
                None,
                "agent",
                &json!({"role": "editor"})
            )
        );
    }
}
