//! Persist a compiled pack: file bytes in the CAS (`packs/`), the pack and its
//! file list in `context_pack` and `context_pack_file` (10 §3.2, §7.4).
//!
//! Packs are immutable and cached on `(version, drive, scope, params,
//! compiler)`: compiling the same request at the same ontology version
//! returns the stored pack. `SHARE.json` is never stored; it belongs to a
//! share and carries a credential.

use serde_json::{Value, json};
use sqlx::{Connection, PgConnection};

use super::{CompiledPack, PackError, pack};
use crate::cas::{Cas, CasKind};

/// A stored pack.
#[derive(Clone, Debug, PartialEq)]
pub struct PackRecord {
    pub id: String,
    pub pack_digest: String,
    pub version: i64,
    pub drive_id: String,
    pub scope: Vec<String>,
    pub manifest: Value,
    pub tokens_brief: i64,
    pub tokens_total: i64,
    pub trimmed: Value,
    pub blocked_facts: i64,
    pub blocked_entities: i64,
    /// RFC 3339 UTC.
    pub created_at: String,
}

impl PackRecord {
    /// The `ContextPack` object (contract `$defs/ContextPack`), without the
    /// BRIEF preview.
    pub fn to_json(&self) -> Value {
        let mut v = self.to_base_json();
        if let Some(c) = self.manifest.get("coverage") {
            v["coverage"] = c.clone();
        }
        if let Some(g) = self.manifest.get("guarantees") {
            v["guarantees"] = g.clone();
        }
        v
    }

    fn to_base_json(&self) -> Value {
        json!({
            "id": self.id,
            "pack_digest": self.pack_digest,
            "version": self.version,
            "drive_id": self.drive_id,
            "scope": self.scope,
            "manifest": self.manifest,
            "tokens": {"brief": self.tokens_brief, "total": self.tokens_total},
            "trimmed": self.trimmed,
            "blocked": {"facts": self.blocked_facts, "entities": self.blocked_entities},
            "created_at": self.created_at,
        })
    }
}

fn store<E: std::fmt::Display>(e: E) -> PackError {
    PackError::Store(e.to_string())
}

const SELECT: &str = "SELECT id::text, pack_digest, version_n, drive_id::text, manifest::text, \
    tokens_brief::bigint, trimmed::text, blocked_facts::bigint, blocked_entities::bigint, \
    to_char(created_at AT TIME ZONE 'UTC', 'YYYY-MM-DD\"T\"HH24:MI:SS.US\"Z\"') \
    FROM _oo_docs.context_pack";

type Row = (
    String,
    String,
    i64,
    String,
    String,
    i64,
    String,
    i64,
    i64,
    String,
);

fn record(row: Row, total: i64) -> Result<PackRecord, PackError> {
    let (id, pack_digest, version, drive_id, manifest, tokens_brief, trimmed, bf, be, created_at) =
        row;
    let manifest: Value = serde_json::from_str(&manifest).map_err(store)?;
    let scope = manifest["scope"]
        .as_array()
        .map(|a| {
            a.iter()
                .filter_map(|v| v.as_str().map(str::to_owned))
                .collect()
        })
        .unwrap_or_default();
    Ok(PackRecord {
        id,
        pack_digest,
        version,
        drive_id,
        scope,
        manifest,
        tokens_brief,
        tokens_total: total,
        trimmed: serde_json::from_str(&trimmed).map_err(store)?,
        blocked_facts: bf,
        blocked_entities: be,
        created_at,
    })
}

async fn total_tokens(conn: &mut PgConnection, id: &str) -> Result<i64, PackError> {
    sqlx::query_scalar(
        "SELECT COALESCE(sum(tokens), 0)::bigint FROM _oo_docs.context_pack_file WHERE pack_id = $1::uuid",
    )
    .bind(id)
    .fetch_one(&mut *conn)
    .await
    .map_err(store)
}

/// Load a pack record.
pub async fn load(conn: &mut PgConnection, pid: &str) -> Result<Option<PackRecord>, PackError> {
    let sql = format!("{SELECT} WHERE id = $1::uuid");
    let row: Option<Row> = sqlx::query_as(sqlx::AssertSqlSafe(sql))
        .bind(pid)
        .fetch_optional(&mut *conn)
        .await
        .map_err(store)?;
    match row {
        Some(r) => {
            let total = total_tokens(conn, &r.0).await?;
            Ok(Some(record(r, total)?))
        }
        None => Ok(None),
    }
}

/// Store `p`; returns the record and whether it was new.
pub async fn persist(
    conn: &mut PgConnection,
    cas: &Cas,
    p: &CompiledPack,
) -> Result<(PackRecord, bool), PackError> {
    let scope_sha = pack::sha256_hex(
        oo_builder::canonical_json(&serde_json::to_value(&p.scope).map_err(store)?).as_bytes(),
    );
    // Cached?
    let sql = format!(
        "{SELECT} WHERE version_n = $1 AND drive_id = $2::uuid AND scope_sha256 = $3 \
         AND params_sha256 = $4 AND compiler = $5"
    );
    let existing: Option<Row> = sqlx::query_as(sqlx::AssertSqlSafe(sql.clone()))
        .bind(p.version.n)
        .bind(&p.drive.id)
        .bind(&scope_sha)
        .bind(&p.params_sha256)
        .bind(super::COMPILER)
        .fetch_optional(&mut *conn)
        .await
        .map_err(store)?;
    // Bytes first, also for a pack already stored: the objects are content
    // addressed, so a rerun is cheap, and a lost or torn object is healed.
    let b3s = cas
        .put_batch(
            CasKind::Pack,
            &p.files
                .iter()
                .map(|f| f.bytes.as_slice())
                .collect::<Vec<_>>(),
            // Derived bytes: the same compile rebuilds them, and a torn file is
            // written again on the next request for the same pack.
            false,
        )
        .map_err(store)?;
    let files: Vec<(&super::pack::PackFile, String)> = p.files.iter().zip(b3s).collect();
    if let Some(r) = existing {
        let total = total_tokens(conn, &r.0).await?;
        return Ok((record(r, total)?, false));
    }
    let mut tx = conn.begin().await.map_err(store)?;
    let id: Option<String> = sqlx::query_scalar(
        "INSERT INTO _oo_docs.context_pack \
           (id, version_n, drive_id, scope_sha256, params_sha256, compiler, pack_digest, \
            snapshot_sha256, manifest, tokens_brief, trimmed, blocked_facts, blocked_entities) \
         VALUES (gen_random_uuid(), $1, $2::uuid, $3, $4, $5, $6, $7, $8::jsonb, $9, $10::jsonb, $11, $12) \
         ON CONFLICT (version_n, drive_id, scope_sha256, params_sha256, compiler) DO NOTHING \
         RETURNING id::text",
    )
    .bind(p.version.n)
    .bind(&p.drive.id)
    .bind(&scope_sha)
    .bind(&p.params_sha256)
    .bind(super::COMPILER)
    .bind(&p.pack_digest)
    .bind(&p.snapshot_sha256)
    .bind(p.manifest.to_string())
    .bind(i32::try_from(p.brief_tokens).unwrap_or(i32::MAX))
    .bind(p.manifest["trimmed"].to_string())
    .bind(i32::try_from(p.blocked.facts).unwrap_or(i32::MAX))
    .bind(i32::try_from(p.blocked.entities).unwrap_or(i32::MAX))
    .fetch_optional(&mut *tx)
    .await
    .map_err(store)?;
    let Some(id) = id else {
        // Lost a race with the same compile: use the winner.
        tx.rollback().await.map_err(store)?;
        let r: Row = sqlx::query_as(sqlx::AssertSqlSafe(sql))
            .bind(p.version.n)
            .bind(&p.drive.id)
            .bind(&scope_sha)
            .bind(&p.params_sha256)
            .bind(super::COMPILER)
            .fetch_one(&mut *conn)
            .await
            .map_err(store)?;
        let total = total_tokens(conn, &r.0).await?;
        return Ok((record(r, total)?, false));
    };
    // One statement for the whole file list.
    let paths: Vec<&str> = files.iter().map(|(f, _)| f.path.as_str()).collect();
    let shas: Vec<String> = files
        .iter()
        .map(|(f, _)| pack::sha256_hex(&f.bytes))
        .collect();
    let bytes: Vec<i32> = files
        .iter()
        .map(|(f, _)| i32::try_from(f.bytes.len()).unwrap_or(i32::MAX))
        .collect();
    let tokens: Vec<i32> = files
        .iter()
        .map(|(f, _)| i32::try_from(f.tokens).unwrap_or(i32::MAX))
        .collect();
    let b3s: Vec<&str> = files.iter().map(|(_, b)| b.as_str()).collect();
    sqlx::query(
        "INSERT INTO _oo_docs.context_pack_file (pack_id, path, sha256, bytes, tokens, cas_b3) \
         SELECT $1::uuid, u.path, u.sha, u.bytes, u.tokens, u.b3 \
           FROM unnest($2::text[], $3::text[], $4::int[], $5::int[], $6::text[]) \
                AS u(path, sha, bytes, tokens, b3)",
    )
    .bind(&id)
    .bind(&paths)
    .bind(&shas)
    .bind(&bytes)
    .bind(&tokens)
    .bind(&b3s)
    .execute(&mut *tx)
    .await
    .map_err(store)?;
    tx.commit().await.map_err(store)?;
    let r = load(conn, &id)
        .await?
        .ok_or(PackError::Internal("pack vanished"))?;
    Ok((r, true))
}

/// Bytes and sha256 of one pack file.
pub async fn read_file(
    conn: &mut PgConnection,
    cas: &Cas,
    pid: &str,
    path: &str,
) -> Result<Option<(Vec<u8>, String)>, PackError> {
    let row: Option<(String, String)> = sqlx::query_as(
        "SELECT cas_b3, sha256 FROM _oo_docs.context_pack_file WHERE pack_id = $1::uuid AND path = $2",
    )
    .bind(pid)
    .bind(path)
    .fetch_optional(&mut *conn)
    .await
    .map_err(store)?;
    let Some((b3, sha)) = row else {
        return Ok(None);
    };
    let bytes = cas.get(CasKind::Pack, &b3).map_err(store)?;
    Ok(Some((bytes, sha)))
}
