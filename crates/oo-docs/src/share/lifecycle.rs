//! Share lifecycle (10 §7.6, D28): create (compile or reuse, share row, refs
//! credential, use records), refresh, revoke, expire, list.
//!
//! `active -> drifted` is the publisher's (`drift`); `-> expired` is the
//! sweeper's (`expire_due`, run from the engine's one-second loop);
//! `-> revoked` and `-> refreshed` are the calls here. A terminal share never
//! moves again. Credentials die with the share in the same transaction.

use serde_json::{Value, json};
use sqlx::{Connection, PgConnection, PgPool};

use crate::api::AppState;
use crate::api::error::{ApiError, code};
use crate::api::http::FMT;
use crate::api::packs::compile_and_store;
use crate::compile::persist::PackRecord;
use crate::compile::{CompileRequest, CompiledPack, normalize_scope};
use crate::share::coverage;
pub use crate::share::coverage::Freshness;
use crate::store::events;

pub const DEFAULT_EXPIRES: i64 = 7 * 24 * 3600;
const B64: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789-_";

/// base64url without padding.
pub fn b64url(bytes: &[u8]) -> String {
    let mut out = String::with_capacity(bytes.len().div_ceil(3) * 4);
    for chunk in bytes.chunks(3) {
        let n = chunk
            .iter()
            .enumerate()
            .fold(0u32, |acc, (i, b)| acc | (u32::from(*b) << (16 - 8 * i)));
        for i in 0..chunk.len() + 1 {
            out.push(B64[((n >> (18 - 6 * i)) & 63) as usize] as char);
        }
    }
    out
}

/// `prefix` + base64url of 32 bytes from the operating system.
pub fn random_token(prefix: &str) -> Result<String, ApiError> {
    use std::io::Read;
    let mut buf = [0u8; 32];
    std::fs::File::open("/dev/urandom")
        .and_then(|mut f| f.read_exact(&mut buf))
        .map_err(|_| ApiError::new(code::INTERNAL, "no randomness source"))?;
    Ok(format!("{prefix}{}", b64url(&buf)))
}

pub fn share_not_found(id: &str) -> ApiError {
    ApiError::new(code::SHARE_NOT_FOUND, "share not found").details(json!({ "share_id": id }))
}

pub fn invalid_transition(id: &str, state: &str, action: &str) -> ApiError {
    ApiError::new(
        code::SHARE_INVALID_TRANSITION,
        format!("a {state} share cannot be {action}"),
    )
    .details(json!({ "share_id": id, "state": state }))
}

/// A share about to be created.
pub struct Plan {
    pub drive: String,
    pub creq: CompileRequest,
    pub fresh: Freshness,
    pub expires_in: i64,
    pub grantor: String,
    /// Task group ticket (`--group`).
    pub group: Option<String>,
    pub refresh_policy: String,
    pub public_host: String,
}

/// A compiled, persisted pack with its credential, ready to insert.
pub struct Prepared {
    pub plan: Plan,
    pub compiled: CompiledPack,
    pub record: PackRecord,
    pub credential: String,
    pub mount_ref: String,
    pub params: Value,
    pub group_id: Option<String>,
    pub group_exists: bool,
}

/// The saved compile request of a share, for refresh.
pub fn params_of(creq: &CompileRequest, drive: &str) -> Value {
    json!({"drive_id": drive, "scope": creq.scope, "task": creq.task, "seeds": creq.seeds,
           "budget": creq.budget, "role": creq.role})
}

/// Coverage, compile, persist. A group that exists pins the version (D56).
pub async fn prepare(app: &AppState, mut plan: Plan) -> Result<Prepared, ApiError> {
    let cas = app
        .cas
        .as_deref()
        .ok_or_else(|| ApiError::new(code::INTERNAL, "this engine has no content store"))?;
    let mut conn = app.pool.acquire().await?;
    let mut group_id = None;
    let mut group_exists = false;
    if let Some(ticket) = &plan.group {
        let row: Option<(String, i64, String, String)> = sqlx::query_as(
            "SELECT id::text, ontology_version, refresh_policy, drive_id::text \
               FROM _oo_docs.share_group WHERE ticket_id = $1",
        )
        .bind(ticket)
        .fetch_optional(&mut *conn)
        .await?;
        if let Some((id, version, policy, drive)) = row {
            if drive != plan.drive {
                return Err(ApiError::bad(
                    "/group",
                    "the group belongs to another drive",
                ));
            }
            plan.creq.version = Some(version);
            plan.refresh_policy = policy;
            group_id = Some(id);
            group_exists = true;
        }
    }
    let scope = normalize_scope(&plan.creq.scope);
    plan.creq.coverage =
        Some(coverage::prepare(&app.pool, &plan.drive, &scope, &plan.fresh).await?);
    let (compiled, record, _) = compile_and_store(&mut conn, cas, &plan.drive, &plan.creq).await?;
    let credential = random_token("oodr_")?;
    let mount_ref = format!(
        "m_{}",
        random_token("")?.chars().take(24).collect::<String>()
    );
    let params = params_of(&plan.creq, &plan.drive);
    Ok(Prepared {
        plan,
        compiled,
        record,
        credential,
        mount_ref,
        params,
        group_id,
        group_exists,
    })
}

/// What `insert` wrote.
pub struct Inserted {
    pub id: String,
    pub created_at: String,
    pub expires_at: String,
}

/// Insert the share, its credential and use records. Run inside the caller's
/// transaction. `refreshed_from` marks the old share `refreshed` and kills its
/// credentials in the same transaction.
pub async fn insert(
    tx: &mut PgConnection,
    p: &Prepared,
    refreshed_from: Option<&str>,
) -> Result<Inserted, ApiError> {
    let drive = &p.plan.drive;
    let mut group_id = p.group_id.clone();
    if let Some(ticket) = &p.plan.group
        && group_id.is_none()
    {
        group_id = sqlx::query_scalar(
            "INSERT INTO _oo_docs.share_group (id, ticket_id, drive_id, ontology_version, refresh_policy) \
             VALUES (gen_random_uuid(), $1, $2::uuid, $3, $4) \
             ON CONFLICT (ticket_id) DO UPDATE SET ticket_id = EXCLUDED.ticket_id \
             RETURNING id::text",
        )
        .bind(ticket)
        .bind(drive)
        .bind(p.compiled.version.n)
        .bind(&p.plan.refresh_policy)
        .fetch_one(&mut *tx)
        .await?;
    }
    let (sid, created, expires): (String, String, String) =
        sqlx::query_as(sqlx::AssertSqlSafe(format!(
            "INSERT INTO _oo_docs.share \
               (id, pack_id, drive_id, scope, role, task, budget, status, grantor, mount_ref, \
                expires_at, group_id, refresh_policy, coverage, compile_params, refreshed_from) \
             VALUES (gen_random_uuid(), $1::uuid, $2::uuid, $3::jsonb, $4, $5, $6, 'active', $7, $8, \
                     clock_timestamp() + $9::bigint * interval '1 second', $10::uuid, $11, \
                     $12::jsonb, $13::jsonb, $14::uuid) \
             RETURNING id::text, to_char(created_at AT TIME ZONE 'UTC', {FMT}), \
                       to_char(expires_at AT TIME ZONE 'UTC', {FMT})"
        )))
        .bind(&p.record.id)
        .bind(drive)
        .bind(serde_json::to_string(&p.compiled.scope).unwrap_or_else(|_| "[]".into()))
        .bind(&p.compiled.role)
        .bind((!p.compiled.task.is_empty()).then_some(p.compiled.task.as_str()))
        .bind(i32::try_from(p.compiled.budget).unwrap_or(i32::MAX))
        .bind(&p.plan.grantor)
        .bind(&p.mount_ref)
        .bind(p.plan.expires_in)
        .bind(&group_id)
        .bind(&p.plan.refresh_policy)
        .bind(p.compiled.coverage.as_ref().map(|c| c.to_json().to_string()))
        .bind(p.params.to_string())
        .bind(refreshed_from)
        .fetch_one(&mut *tx)
        .await?;
    sqlx::query(
        "INSERT INTO _oo_docs.refs_credential (id, share_id, token_b3, scopes, expires_at) \
         SELECT gen_random_uuid(), $1::uuid, $2, ARRAY['refs:read'], expires_at \
           FROM _oo_docs.share WHERE id = $1::uuid",
    )
    .bind(&sid)
    .bind(blake3::hash(p.credential.as_bytes()).to_hex().to_string())
    .execute(&mut *tx)
    .await?;
    let keys: Vec<String> = p.compiled.uses.iter().map(|u| u.fact_key.clone()).collect();
    let sections: Vec<String> = p
        .compiled
        .uses
        .iter()
        .map(|u| u.section.to_owned())
        .collect();
    let precisions: Vec<String> = p
        .compiled
        .uses
        .iter()
        .map(|u| u.precision.to_owned())
        .collect();
    sqlx::query(
        "INSERT INTO _oo_docs.share_fact_use (share_id, fact_id, section, precision) \
         SELECT DISTINCT ON (f.id) $1::uuid, f.id, u.section, u.precision \
           FROM unnest($2::text[], $3::text[], $4::text[]) AS u(fact_key, section, precision) \
           JOIN _oo_docs.fact f ON f.fact_key = u.fact_key \
          WHERE f.status IN ('published', 'retracted') AND f.published_in <= $5 \
            AND (f.retracted_in IS NULL OR f.retracted_in > $5) \
          ORDER BY f.id \
         ON CONFLICT DO NOTHING",
    )
    .bind(&sid)
    .bind(&keys)
    .bind(&sections)
    .bind(&precisions)
    .bind(p.compiled.version.n)
    .execute(&mut *tx)
    .await?;
    let su: Vec<&(String, String, String)> = p.compiled.section_uses.iter().collect();
    sqlx::query(
        "INSERT INTO _oo_docs.share_section_use (share_id, section, fact_key, section_digest) \
         SELECT $1::uuid, u.section, u.fact_key, u.digest \
           FROM unnest($2::text[], $3::text[], $4::text[]) AS u(section, fact_key, digest) \
         ON CONFLICT DO NOTHING",
    )
    .bind(&sid)
    .bind(su.iter().map(|x| x.0.clone()).collect::<Vec<_>>())
    .bind(su.iter().map(|x| x.1.clone()).collect::<Vec<_>>())
    .bind(su.iter().map(|x| x.2.clone()).collect::<Vec<_>>())
    .execute(&mut *tx)
    .await?;
    // A share pinned to an older version (a task group member) gets the entries of the versions it
    // missed, in the same transaction, so a change that came before it existed still reaches it.
    super::drift::backfill(tx, &sid).await?;
    if let Some(old) = refreshed_from {
        sqlx::query(
            "UPDATE _oo_docs.share SET status = 'refreshed', revision = revision + 1 WHERE id = $1::uuid",
        )
        .bind(old)
        .execute(&mut *tx)
        .await?;
        sqlx::query(
            "UPDATE _oo_docs.refs_credential SET revoked_at = clock_timestamp() \
              WHERE share_id = $1::uuid AND revoked_at IS NULL",
        )
        .bind(old)
        .execute(&mut *tx)
        .await?;
        events::append_event(
            tx,
            "share.state_changed",
            Some(drive),
            Some(old),
            &json!({"share_id": old, "to": "refreshed", "refreshed_to": sid}).to_string(),
        )
        .await?;
    }
    events::append_event(
        tx,
        "share.created",
        Some(drive),
        Some(&sid),
        &json!({"share_id": sid, "pack_id": p.record.id, "expires_at": expires,
                "refreshed_from": refreshed_from})
        .to_string(),
    )
    .await?;
    Ok(Inserted {
        id: sid,
        created_at: created,
        expires_at: expires,
    })
}

/// The creation payload: the `Share`, plus the pack file list, `SHARE.json`
/// text (holds the one-time credential), sizes and counts.
pub async fn payload(
    conn: &mut PgConnection,
    p: &Prepared,
    ins: &Inserted,
) -> Result<Value, ApiError> {
    use crate::compile::pack::{ShareFields, sha256_hex};
    let share_file = p.compiled.share_json(&ShareFields {
        share_id: ins.id.clone(),
        grantor: p.plan.grantor.clone(),
        created_at: ins.created_at.clone(),
        expires_at: ins.expires_at.clone(),
        refs_endpoint: format!("http://{}/v1/docs/refs", p.plan.public_host),
        refs_credential: p.credential.clone(),
        refs_expires_at: ins.expires_at.clone(),
    });
    let mut share = share_value(conn, &ins.id)
        .await?
        .ok_or_else(|| share_not_found(&ins.id))?;
    share["files"] = Value::Array(
        p.compiled
            .files
            .iter()
            .map(|f| {
                json!({"path": f.path, "sha256": sha256_hex(&f.bytes), "bytes": f.bytes.len(),
                       "tokens": f.tokens})
            })
            .collect(),
    );
    share["share_json"] = Value::String(share_file.as_str().to_owned());
    share["mount_ref"] = Value::String(p.mount_ref.clone());
    share["tokens"] = json!({"brief": p.record.tokens_brief, "budget": p.compiled.budget});
    share["trimmed"] = p.record.trimmed.clone();
    share["blocked"] =
        json!({"facts": p.record.blocked_facts, "entities": p.record.blocked_entities});
    Ok(share)
}

const TS: &str = "'YYYY-MM-DD\"T\"HH24:MI:SS.US\"Z\"'";

fn share_sql() -> String {
    format!(
        "SELECT s.id::text AS id, s.pack_id::text AS pack_id, p.pack_digest, s.drive_id::text AS drive, \
                s.scope::text AS scope, s.task, s.budget, s.role, s.status, \
                to_char(s.created_at AT TIME ZONE 'UTC', {TS}) AS created, \
                to_char(s.expires_at AT TIME ZONE 'UTC', {TS}) AS expires, \
                to_char(s.revoked_at AT TIME ZONE 'UTC', {TS}) AS revoked, \
                s.refreshed_from::text AS refreshed_from, s.revision, p.version_n, \
                s.group_id::text AS group_id, s.refresh_policy, s.coverage::text AS coverage, \
                p.manifest -> 'guarantees' ->> 'constraint_free' AS cfree \
           FROM _oo_docs.share s JOIN _oo_docs.context_pack p ON p.id = s.pack_id"
    )
}

fn row_value(r: &sqlx::postgres::PgRow) -> Result<Value, sqlx::Error> {
    use sqlx::Row;
    let cfree: Option<String> = r.try_get("cfree")?;
    let coverage: Option<String> = r.try_get("coverage")?;
    let mut guarantees = json!({
        "pack_consistent": true, "freshness_floor": true, "change_delivery": true,
        "group_same_version": true,
        "constraint_free": cfree.as_deref() != Some("false"),
    });
    let cov: Value = coverage
        .as_deref()
        .and_then(|c| serde_json::from_str(c).ok())
        .unwrap_or(Value::Null);
    if let Some(c) = cov.as_object() {
        let empty = |k: &str| c.get(k).and_then(Value::as_array).is_none_or(Vec::is_empty);
        let complete = empty("pending")
            && empty("failed")
            && c.get("covered_rev").and_then(Value::as_i64)
                >= c.get("drive_rev").and_then(Value::as_i64);
        guarantees["freshness_floor"] = json!(complete);
    }
    let scope: String = r.try_get("scope")?;
    let mut v = json!({
        "id": r.try_get::<String, _>("id")?, "pack_id": r.try_get::<String, _>("pack_id")?,
        "pack_digest": r.try_get::<String, _>("pack_digest")?,
        "drive_id": r.try_get::<String, _>("drive")?,
        "scope": serde_json::from_str::<Value>(&scope).unwrap_or(Value::Null),
        "role": r.try_get::<String, _>("role")?, "task": r.try_get::<Option<String>, _>("task")?,
        "budget": r.try_get::<i32, _>("budget")?, "version": r.try_get::<i64, _>("version_n")?,
        "state": r.try_get::<String, _>("status")?,
        "mount": {"present": false},
        "refreshed_from": r.try_get::<Option<String>, _>("refreshed_from")?,
        "created_at": r.try_get::<String, _>("created")?,
        "expires_at": r.try_get::<String, _>("expires")?,
        "revoked_at": r.try_get::<Option<String>, _>("revoked")?,
        "revision": r.try_get::<i64, _>("revision")?,
        "group_id": r.try_get::<Option<String>, _>("group_id")?,
        "refresh_policy": r.try_get::<String, _>("refresh_policy")?,
        "guarantees": guarantees,
    });
    if !cov.is_null() {
        v["coverage"] = cov;
    }
    Ok(v)
}

/// The `Share` object of `sid`, with `drift` filled for a drifted share.
pub async fn share_value(conn: &mut PgConnection, sid: &str) -> Result<Option<Value>, ApiError> {
    let row = sqlx::query(sqlx::AssertSqlSafe(format!(
        "{} WHERE s.id = $1::uuid",
        share_sql()
    )))
    .bind(sid)
    .fetch_optional(&mut *conn)
    .await?;
    let Some(row) = row else { return Ok(None) };
    let mut v = row_value(&row)?;
    if v["state"] == "drifted" {
        let facts: i64 = sqlx::query_scalar(
            "SELECT count(DISTINCT k) FROM _oo_docs.share_update u, unnest(u.fact_keys) k \
              WHERE u.share_id = $1::uuid AND u.level IN ('warn', 'block')",
        )
        .bind(sid)
        .fetch_one(&mut *conn)
        .await?;
        v["drift"] =
            json!({"facts_changed": facts, "files_changed": 0, "since_version": v["version"]});
    }
    Ok(Some(v))
}

/// A page of shares, newest first. Filters: `state`, `drive_id`, `group_id`.
pub async fn list(
    conn: &mut PgConnection,
    state: Option<&str>,
    drive: Option<&str>,
    group: Option<&str>,
    after: Option<(String, String)>,
    limit: i64,
) -> Result<(Vec<Value>, Option<(String, String)>), ApiError> {
    let (after_ts, after_id) = after.unzip();
    let rows: Vec<(String, String)> = sqlx::query_as(
        "SELECT s.id::text, to_char(s.created_at AT TIME ZONE 'UTC', 'YYYY-MM-DD\"T\"HH24:MI:SS.US\"Z\"') \
           FROM _oo_docs.share s \
          WHERE ($1::text IS NULL OR s.status = $1) AND ($2::uuid IS NULL OR s.drive_id = $2::uuid) \
            AND ($3::uuid IS NULL OR s.group_id = $3::uuid) \
            AND ($4::text IS NULL OR (s.created_at, s.id) < ($4::timestamptz, $5::uuid)) \
          ORDER BY s.created_at DESC, s.id DESC LIMIT $6",
    )
    .bind(state)
    .bind(drive)
    .bind(group)
    .bind(after_ts)
    .bind(after_id)
    .bind(limit + 1)
    .fetch_all(&mut *conn)
    .await?;
    let more = rows.len() as i64 > limit;
    let mut items = Vec::new();
    let mut last = None;
    for (id, ts) in rows.into_iter().take(limit as usize) {
        if let Some(v) = share_value(conn, &id).await? {
            items.push(v);
        }
        last = Some((ts, id));
    }
    Ok((items, if more { last } else { None }))
}

/// Revoke: `active` or `drifted` to `revoked`, credentials dead in the same
/// transaction. Returns the share.
pub async fn revoke(conn: &mut PgConnection, sid: &str, reason: &str) -> Result<Value, ApiError> {
    let mut tx = conn.begin().await?;
    let row: Option<(String, String)> = sqlx::query_as(
        "SELECT status, drive_id::text FROM _oo_docs.share WHERE id = $1::uuid FOR UPDATE",
    )
    .bind(sid)
    .fetch_optional(&mut *tx)
    .await?;
    let Some((status, drive)) = row else {
        return Err(share_not_found(sid));
    };
    if !matches!(status.as_str(), "active" | "drifted") {
        return Err(invalid_transition(sid, &status, "revoked"));
    }
    sqlx::query(
        "UPDATE _oo_docs.share SET status = 'revoked', revoked_at = clock_timestamp(), \
                revoke_reason = $2, revision = revision + 1 WHERE id = $1::uuid",
    )
    .bind(sid)
    .bind(reason)
    .execute(&mut *tx)
    .await?;
    sqlx::query(
        "UPDATE _oo_docs.refs_credential SET revoked_at = clock_timestamp() \
          WHERE share_id = $1::uuid AND revoked_at IS NULL",
    )
    .bind(sid)
    .execute(&mut *tx)
    .await?;
    events::append_event(
        &mut tx,
        "share.state_changed",
        Some(&drive),
        Some(sid),
        &json!({"share_id": sid, "from": status, "to": "revoked", "reason": reason}).to_string(),
    )
    .await?;
    tx.commit().await?;
    share_value(conn, sid)
        .await?
        .ok_or_else(|| share_not_found(sid))
}

/// Sweep `expires_at <= now()`: terminal `expired`, credentials revoked, one
/// event each (the daemon deletes the mount when it sees it). Returns the
/// number of shares expired.
pub async fn expire_due(pool: &PgPool) -> Result<usize, ApiError> {
    let due: Vec<String> = sqlx::query_scalar(
        "SELECT id::text FROM _oo_docs.share WHERE status IN ('active', 'drifted') \
            AND expires_at <= clock_timestamp() ORDER BY expires_at LIMIT 200",
    )
    .fetch_all(pool)
    .await?;
    let mut n = 0;
    for sid in due {
        let mut tx = pool.begin().await?;
        let row: Option<(String, String)> = sqlx::query_as(
            "UPDATE _oo_docs.share SET status = 'expired', revision = revision + 1 \
              WHERE id = $1::uuid AND status IN ('active', 'drifted') \
                AND expires_at <= clock_timestamp() RETURNING drive_id::text, status",
        )
        .bind(&sid)
        .fetch_optional(&mut *tx)
        .await?;
        let Some((drive, _)) = row else {
            tx.rollback().await?;
            continue;
        };
        sqlx::query(
            "UPDATE _oo_docs.refs_credential SET revoked_at = clock_timestamp() \
              WHERE share_id = $1::uuid AND revoked_at IS NULL",
        )
        .bind(&sid)
        .execute(&mut *tx)
        .await?;
        events::append_event(
            &mut tx,
            "share.state_changed",
            Some(&drive),
            Some(&sid),
            &json!({"share_id": sid, "to": "expired"}).to_string(),
        )
        .await?;
        tx.commit().await?;
        n += 1;
    }
    Ok(n)
}

/// What `refresh` needs of a share.
pub struct Old {
    pub id: String,
    pub drive: String,
    pub status: String,
    pub group_id: Option<String>,
    pub policy: String,
    pub params: Value,
    pub expires_in: i64,
    pub grantor: String,
}

/// `(id, drive, status, group, policy, compile params, expires_in, grantor)`.
type OldRow = (
    String,
    String,
    String,
    Option<String>,
    String,
    Option<String>,
    i64,
    String,
);

/// Read a share's saved parameters.
pub async fn load_old(conn: &mut PgConnection, sid: &str) -> Result<Old, ApiError> {
    let r: Option<OldRow> = sqlx::query_as(
        "SELECT id::text, drive_id::text, status, group_id::text, refresh_policy, \
                    compile_params::text, \
                    GREATEST(3600, EXTRACT(EPOCH FROM (expires_at - created_at))::bigint), grantor \
               FROM _oo_docs.share WHERE id = $1::uuid",
    )
    .bind(sid)
    .fetch_optional(&mut *conn)
    .await?;
    let Some((id, drive, status, group_id, policy, params, expires_in, grantor)) = r else {
        return Err(share_not_found(sid));
    };
    Ok(Old {
        id,
        drive,
        status,
        group_id,
        policy,
        params: params
            .and_then(|p| serde_json::from_str(&p).ok())
            .unwrap_or(Value::Null),
        expires_in,
        grantor,
    })
}

/// The compile request a share was made from, at the latest version.
pub fn request_of(params: &Value) -> CompileRequest {
    let strings = |k: &str| -> Vec<String> {
        params[k]
            .as_array()
            .map(|a| {
                a.iter()
                    .filter_map(|x| x.as_str().map(str::to_owned))
                    .collect()
            })
            .unwrap_or_default()
    };
    CompileRequest {
        scope: strings("scope"),
        seeds: strings("seeds"),
        task: params["task"].as_str().unwrap_or("").to_owned(),
        budget: params["budget"]
            .as_u64()
            .and_then(|b| u32::try_from(b).ok())
            .unwrap_or(crate::compile::DEFAULT_BUDGET),
        role: params["role"].as_str().unwrap_or("suggester").to_owned(),
        version: None,
        coverage: None,
    }
}

/// `POST /shares/{sid}:refresh`: same parameters, latest version; the old
/// share becomes `refreshed`. Group members refresh with their group only.
pub async fn refresh(app: &AppState, sid: &str, public_host: &str) -> Result<Value, ApiError> {
    let old = {
        let mut conn = app.pool.acquire().await?;
        load_old(&mut conn, sid).await?
    };
    if !matches!(old.status.as_str(), "active" | "drifted") {
        return Err(invalid_transition(sid, &old.status, "refreshed"));
    }
    if old.policy == "pinned" {
        return Err(
            ApiError::new(code::SHARE_PINNED, "a pinned share does not accept refresh")
                .details(json!({"share_id": sid})),
        );
    }
    if old.group_id.is_some() {
        return Err(ApiError::new(
            code::SHARE_GROUP_MEMBER,
            "a member of a task group refreshes with its group",
        )
        .details(json!({"share_id": sid, "group_id": old.group_id})));
    }
    let prepared = prepare(
        app,
        Plan {
            drive: old.drive.clone(),
            creq: request_of(&old.params),
            fresh: Freshness {
                wait_ms: 0,
                ..Freshness::default()
            },
            expires_in: old.expires_in,
            grantor: old.grantor.clone(),
            group: None,
            refresh_policy: old.policy.clone(),
            public_host: public_host.to_owned(),
        },
    )
    .await?;
    let mut conn = app.pool.acquire().await?;
    let mut tx = conn.begin().await?;
    let locked: Option<String> =
        sqlx::query_scalar("SELECT status FROM _oo_docs.share WHERE id = $1::uuid FOR UPDATE")
            .bind(sid)
            .fetch_optional(&mut *tx)
            .await?;
    match locked.as_deref() {
        Some("active" | "drifted") => {}
        Some(other) => return Err(invalid_transition(sid, other, "refreshed")),
        None => return Err(share_not_found(sid)),
    }
    let ins = insert(&mut tx, &prepared, Some(sid)).await?;
    tx.commit().await?;
    payload(&mut conn, &prepared, &ins).await
}
