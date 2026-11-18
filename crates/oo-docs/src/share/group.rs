//! Task groups (D56, 10 §16.6): shares of one ticket pin one ontology version
//! and refresh together, all or none.

use serde_json::{Value, json};
use sqlx::Connection;

use super::lifecycle::{self, Freshness, Plan};
use crate::api::AppState;
use crate::api::error::{ApiError, code};
use crate::api::http::is_uuid;
use crate::compile::CompileRequest;

/// `gid` is the group's id or its ticket.
async fn find(
    conn: &mut sqlx::PgConnection,
    gid: &str,
) -> Result<(String, String, String), ApiError> {
    let row: Option<(String, String, String)> = sqlx::query_as(
        "SELECT id::text, refresh_policy, drive_id::text FROM _oo_docs.share_group \
          WHERE ($2 AND id = $1::uuid) OR (NOT $2 AND ticket_id = $1)",
    )
    .bind(gid)
    .bind(is_uuid(gid))
    .fetch_optional(&mut *conn)
    .await?;
    row.ok_or_else(|| {
        ApiError::new(code::NOT_FOUND, "share group not found")
            .details(json!({ "group": gid, "resource": "share_group" }))
    })
}

/// `GET /share-groups/{gid}`: the group and its members' versions.
pub async fn get(conn: &mut sqlx::PgConnection, gid: &str) -> Result<Value, ApiError> {
    let (id, policy, drive) = find(conn, gid).await?;
    let (ticket, version, created): (String, i64, String) = sqlx::query_as(
        "SELECT ticket_id, ontology_version, \
                to_char(created_at AT TIME ZONE 'UTC', 'YYYY-MM-DD\"T\"HH24:MI:SS.US\"Z\"') \
           FROM _oo_docs.share_group WHERE id = $1::uuid",
    )
    .bind(&id)
    .fetch_one(&mut *conn)
    .await?;
    let members: Vec<(String, String, i64)> = sqlx::query_as(
        "SELECT s.id::text, s.status, p.version_n FROM _oo_docs.share s \
           JOIN _oo_docs.context_pack p ON p.id = s.pack_id WHERE s.group_id = $1::uuid \
          ORDER BY s.created_at, s.id",
    )
    .bind(&id)
    .fetch_all(&mut *conn)
    .await?;
    Ok(json!({
        "id": id, "ticket_id": ticket, "drive_id": drive, "ontology_version": version,
        "refresh_policy": policy, "created_at": created,
        "members": members.iter().map(|(i, s, v)| json!({"share_id": i, "state": s, "version": v})).collect::<Vec<_>>(),
    }))
}

/// Refresh every live member to the latest version in one transaction.
/// Returns `{results, refreshed}`; `refreshed[i]` is the creation payload of
/// the new share of `results[i]`.
pub async fn refresh(app: &AppState, gid: &str, public_host: &str) -> Result<Value, ApiError> {
    let (id, policy, _drive, members) = {
        let mut conn = app.pool.acquire().await?;
        let (id, policy, drive) = find(&mut conn, gid).await?;
        let members: Vec<String> = sqlx::query_scalar(
            "SELECT id::text FROM _oo_docs.share WHERE group_id = $1::uuid \
                AND status IN ('active', 'drifted') ORDER BY created_at, id",
        )
        .bind(&id)
        .fetch_all(&mut *conn)
        .await?;
        (id, policy, drive, members)
    };
    if policy == "pinned" {
        return Err(
            ApiError::new(code::SHARE_PINNED, "a pinned group does not accept refresh")
                .details(json!({"group_id": id})),
        );
    }
    if members.is_empty() {
        return Err(ApiError::new(
            code::SHARE_INVALID_TRANSITION,
            "the group has no live share",
        )
        .details(json!({"group_id": id})));
    }
    let latest: i64 = {
        let mut conn = app.pool.acquire().await?;
        sqlx::query_scalar("SELECT COALESCE(max(n), 0) FROM _oo_docs.ontology_version")
            .fetch_one(&mut *conn)
            .await?
    };
    // Compile every new pack first (content addressed, harmless to repeat);
    // nothing else changes until all of them exist.
    let mut prepared = Vec::new();
    let mut olds = Vec::new();
    for sid in &members {
        let old = {
            let mut conn = app.pool.acquire().await?;
            lifecycle::load_old(&mut conn, sid).await?
        };
        let mut creq: CompileRequest = lifecycle::request_of(&old.params);
        creq.version = Some(latest);
        {
            let mut conn = app.pool.acquire().await?;
            creq.changes = crate::share::changes::compute(&mut conn, sid, Some(latest)).await?;
        }
        let mut p = lifecycle::prepare(
            app,
            Plan {
                drive: old.drive.clone(),
                creq,
                fresh: Freshness {
                    wait_ms: 0,
                    ..Freshness::default()
                },
                expires_in: old.expires_in,
                grantor: old.grantor.clone(),
                group: None,
                refresh_policy: policy.clone(),
                public_host: public_host.to_owned(),
            },
        )
        .await
        .map_err(|mut e| {
            e.details = json!({"share_id": sid, "group_id": id, "cause": e.details});
            e
        })?;
        p.group_id = Some(id.clone());
        prepared.push(p);
        olds.push(old);
    }
    let mut conn = app.pool.acquire().await?;
    let mut tx = conn.begin().await?;
    sqlx::query("SELECT id FROM _oo_docs.share_group WHERE id = $1::uuid FOR UPDATE")
        .bind(&id)
        .fetch_one(&mut *tx)
        .await?;
    let locked: Vec<(String, String)> = sqlx::query_as(
        "SELECT id::text, status FROM _oo_docs.share WHERE id = ANY($1::text[]::uuid[]) \
          ORDER BY id FOR UPDATE",
    )
    .bind(&members)
    .fetch_all(&mut *tx)
    .await?;
    if let Some((bad, st)) = locked
        .iter()
        .find(|(_, st)| !matches!(st.as_str(), "active" | "drifted"))
    {
        return Err(lifecycle::invalid_transition(bad, st, "refreshed"));
    }
    let mut inserted = Vec::new();
    for (p, old) in prepared.iter().zip(&olds) {
        inserted.push(lifecycle::insert(&mut tx, p, Some(&old.id)).await?);
    }
    sqlx::query(
        "UPDATE _oo_docs.share_group SET ontology_version = $2, revision = revision + 1 WHERE id = $1::uuid",
    )
    .bind(&id)
    .bind(latest)
    .execute(&mut *tx)
    .await?;
    tx.commit().await?;
    let mut results = Vec::new();
    let mut refreshed = Vec::new();
    for ((p, old), ins) in prepared.iter().zip(&olds).zip(&inserted) {
        results.push(json!({"id": old.id, "status": "ok", "new_share_id": ins.id}));
        refreshed.push(lifecycle::payload(&mut conn, p, ins).await?);
    }
    Ok(json!({"results": results, "refreshed": refreshed, "ontology_version": latest}))
}
