//! `file`, `file_version` and `blob` writes shared by reconcile and hashing.
//!
//! Lock order follows 10 §4.1.3: job rows, then the `file` row, then stage
//! products, `append_event` last (it takes the `event_head` lock until
//! commit). Each helper here that appends an event does so as its final
//! statement, and a transaction that touches more than one pre-existing file
//! row must take all row locks before its first event.

use oo_connectors::objectstore::Fingerprint;
use sqlx::{PgConnection, Row};

use super::{IngestError, fp_sha256, hex, unhex};
use crate::jobs::{JobKind, JobStatus, NewJob};
use crate::state::{self, FileState, Transition};
use crate::store::{events, jobs as store};

/// One `file` row with the current version's fingerprint.
#[derive(Clone, Debug)]
pub struct FileRow {
    /// File id.
    pub id: String,
    /// Source key.
    pub raw_key: Vec<u8>,
    /// Normalized path.
    pub path: String,
    /// `file.state` text.
    pub state: String,
    /// `file.state_code`.
    pub state_code: Option<String>,
    /// Row carries `deleted_at`.
    pub deleted: bool,
    /// Current version id.
    pub version_id: Option<String>,
    /// Current version blob.
    pub b3: Option<String>,
    /// Fingerprint stored with the current version.
    pub fp: Option<Fingerprint>,
    /// `last_seen_generation`.
    pub seen: i64,
}

/// Every file row of a Drive, deleted ones included (a deleted raw key that
/// reappears reuses its row, `UNIQUE (drive_id, raw_key)`).
pub async fn load_files(
    conn: &mut PgConnection,
    drive_id: &str,
) -> Result<Vec<FileRow>, IngestError> {
    let rows = sqlx::query(
        "SELECT f.id::text AS id, encode(f.raw_key, 'hex') AS raw_hex, f.path, f.state, \
         f.state_code, (f.deleted_at IS NOT NULL) AS deleted, f.current_version_id::text AS vid, \
         cv.b3 AS b3, cv.fp::text AS fp, f.last_seen_generation AS seen, f.fp_size, f.fp_mtime_ns, \
         f.fp_ino, f.fp_etag, f.fp_version_id \
         FROM _oo_docs.file f LEFT JOIN _oo_docs.file_version cv ON cv.id = f.current_version_id \
         WHERE f.drive_id = $1::uuid",
    )
    .bind(drive_id)
    .fetch_all(&mut *conn)
    .await?;
    let mut out = Vec::with_capacity(rows.len());
    for r in &rows {
        let raw_hex: String = r.try_get("raw_hex")?;
        let fp: Option<String> = r.try_get("fp")?;
        let fp_size: Option<i64> = r.try_get("fp_size")?;
        let from_version: Option<Fingerprint> = fp.and_then(|t| serde_json::from_str(&t).ok());
        let fp = from_version.or_else(|| {
            fp_size.map(|size| Fingerprint {
                size: size.max(0) as u64,
                etag: r.try_get("fp_etag").ok().flatten(),
                last_modified_ns: r
                    .try_get::<Option<i64>, _>("fp_mtime_ns")
                    .ok()
                    .flatten()
                    .map(i128::from),
                version_id: r.try_get("fp_version_id").ok().flatten(),
                fs_ino: r
                    .try_get::<Option<i64>, _>("fp_ino")
                    .ok()
                    .flatten()
                    .map(|i| i as u64),
                fs_ctime_ns: None,
            })
        });
        out.push(FileRow {
            id: r.try_get("id")?,
            raw_key: unhex(&raw_hex).ok_or_else(|| IngestError::Protocol("raw_key hex".into()))?,
            path: r.try_get("path")?,
            state: r.try_get("state")?,
            state_code: r.try_get("state_code")?,
            deleted: r.try_get("deleted")?,
            version_id: r.try_get("vid")?,
            b3: r.try_get("b3")?,
            fp,
            seen: r.try_get("seen")?,
        });
    }
    Ok(out)
}

fn state_payload(file_id: &str, from: Option<&str>, to: &str, code: Option<&str>) -> String {
    let q = |s: Option<&str>| match s {
        Some(v) => format!("\"{}\"", v.replace(['\\', '"'], "")),
        None => "null".to_owned(),
    };
    format!(
        "{{\"file_id\":\"{file_id}\",\"from\":{},\"to\":\"{to}\",\"code\":{}}}",
        q(from),
        q(code)
    )
}

/// Insert a new file in state `stored` with history row 1 and the creation
/// event. `None` when `(drive, raw_key)` already exists (a concurrent
/// creator won). `seen` is the generation it counts as seen in.
pub async fn insert_file(
    conn: &mut PgConnection,
    drive_id: &str,
    raw_key: &[u8],
    path: &str,
    path_fold: &str,
    seen: i64,
) -> Result<Option<String>, IngestError> {
    let id: Option<String> = sqlx::query_scalar(
        "INSERT INTO _oo_docs.file (id, drive_id, path, raw_key, path_fold, state, last_seen_generation) \
         VALUES (gen_random_uuid(), $1::uuid, $2, $3, $4, 'stored', $5) \
         ON CONFLICT (drive_id, raw_key) DO NOTHING RETURNING id::text",
    )
    .bind(drive_id)
    .bind(path)
    .bind(raw_key)
    .bind(path_fold)
    .bind(seen)
    .fetch_optional(&mut *conn)
    .await?;
    let Some(id) = id else { return Ok(None) };
    sqlx::query(
        "INSERT INTO _oo_docs.file_state_history (file_id, seq, from_state, to_state) \
         VALUES ($1::uuid, 1, NULL, 'stored')",
    )
    .bind(&id)
    .execute(&mut *conn)
    .await?;
    events::append_event(
        conn,
        "file.state_changed",
        Some(drive_id),
        Some(&id),
        &state_payload(&id, None, "stored", None),
    )
    .await?;
    Ok(Some(id))
}

/// Mark files as seen in `generation` (before any event is appended).
pub async fn touch(
    conn: &mut PgConnection,
    ids: &[String],
    generation: i64,
) -> Result<(), IngestError> {
    if ids.is_empty() {
        return Ok(());
    }
    sqlx::query(
        "UPDATE _oo_docs.file SET last_seen_generation = $2 \
         WHERE id = ANY($1::text[]::uuid[]) AND last_seen_generation < $2",
    )
    .bind(ids)
    .bind(generation)
    .execute(&mut *conn)
    .await?;
    Ok(())
}

/// Enqueue the hash job for a file at a fingerprint. The key is
/// `hash:<file_id>:<fp_sha256>`; when that key already finished (the file
/// went back to an older fingerprint, or the job failed for good) the
/// generation and a counter are appended, as 4.1.2 asks for `retry#n`.
pub async fn enqueue_hash(
    conn: &mut PgConnection,
    drive_id: &str,
    file_id: &str,
    fp: &Fingerprint,
    generation: i64,
    priority: i16,
) -> Result<JobStatus, IngestError> {
    let base = format!("hash:{file_id}:{}", fp_sha256(fp));
    let mut candidates = vec![base.clone(), format!("{base}#g{generation}")];
    candidates.extend((1..=4).map(|n| format!("{base}#g{generation}r{n}")));
    for key in &candidates {
        let mut job = NewJob::new(JobKind::Hash, key);
        job.drive_id = Some(drive_id);
        job.file_id = Some(file_id);
        job.priority = priority;
        let res = store::enqueue(conn, &job).await?;
        match res.status {
            JobStatus::Enqueued | JobStatus::Replayed => return Ok(res.status),
            JobStatus::AlreadyDone | JobStatus::Terminal => continue,
            other => return Ok(other),
        }
    }
    Ok(JobStatus::Terminal)
}

/// Cancel every queued or running job of a file (4.1.5): a deleted file or a
/// new version makes them pointless. Queued ones become `cancelled`, running
/// ones get `cancel_requested`. `except` keeps one job (the caller's own);
/// `include_hash` is false when a newer fingerprint may already be queued.
pub async fn cancel_file_jobs(
    conn: &mut PgConnection,
    file_id: &str,
    except: Option<&str>,
    include_hash: bool,
) -> Result<u32, IngestError> {
    let ids: Vec<String> = sqlx::query_scalar(
        "SELECT id::text FROM _oo_docs.job WHERE file_id = $1::uuid \
         AND state IN ('queued', 'running') AND ($2 OR kind <> 'hash') ORDER BY id",
    )
    .bind(file_id)
    .bind(include_hash)
    .fetch_all(&mut *conn)
    .await?;
    let mut n = 0;
    for id in ids.iter().filter(|i| Some(i.as_str()) != except) {
        store::cancel(conn, id).await?;
        n += 1;
    }
    Ok(n)
}

/// Take the file row lock and return its state text, `None` when gone.
pub async fn lock_file(
    conn: &mut PgConnection,
    file_id: &str,
) -> Result<Option<(String, bool)>, IngestError> {
    let row: Option<(String, bool)> = sqlx::query_as(
        "SELECT state, deleted_at IS NOT NULL FROM _oo_docs.file WHERE id = $1::uuid FOR UPDATE",
    )
    .bind(file_id)
    .fetch_optional(&mut *conn)
    .await?;
    Ok(row)
}

/// Move a file with a live or dead version back to `stored` for a new
/// version or a new verdict: `X -> superseded -> stored` (or `deleted ->
/// stored` on reappearance), per the 10 §4.2 table. The caller holds the file
/// lock and appends nothing else before this returns.
pub async fn restart_as_stored(
    conn: &mut PgConnection,
    file_id: &str,
    from: FileState,
    version_id: Option<&str>,
) -> Result<(), IngestError> {
    if !matches!(from, FileState::Superseded | FileState::Deleted) {
        state::transition_in_tx(
            conn,
            &Transition {
                file_id,
                expect_from: Some(from),
                to: FileState::Superseded,
                code: None,
                retryable: false,
                version_id,
                attempt: None,
                detail_json: None,
            },
        )
        .await?;
    }
    let now_from = if from == FileState::Deleted {
        FileState::Deleted
    } else {
        FileState::Superseded
    };
    state::transition_in_tx(
        conn,
        &Transition {
            file_id,
            expect_from: Some(now_from),
            to: FileState::Stored,
            code: None,
            retryable: false,
            version_id: None,
            attempt: None,
            detail_json: None,
        },
    )
    .await?;
    Ok(())
}

/// Optional parts of a transition.
#[derive(Clone, Copy, Debug, Default)]
pub struct Extra<'a> {
    /// `state_code`.
    pub code: Option<&'a str>,
    /// `file.retryable`.
    pub retryable: bool,
    /// Version the change belongs to.
    pub version_id: Option<&'a str>,
    /// JSON object text for the history row.
    pub detail_json: Option<&'a str>,
}

impl<'a> Extra<'a> {
    /// Just a code.
    pub fn code(code: Option<&'a str>) -> Self {
        Self {
            code,
            ..Self::default()
        }
    }
}

/// Move a file to a verdict state.
pub async fn to_state(
    conn: &mut PgConnection,
    file_id: &str,
    from: FileState,
    to: FileState,
    extra: Extra<'_>,
) -> Result<(), IngestError> {
    state::transition_in_tx(
        conn,
        &Transition {
            file_id,
            expect_from: Some(from),
            to,
            code: extra.code,
            retryable: extra.retryable,
            version_id: extra.version_id,
            attempt: None,
            detail_json: extra.detail_json,
        },
    )
    .await?;
    Ok(())
}

/// Withdraw the evidence of a version and retract facts left without any
/// live evidence unless a human asserted them (4.3.5). Returns
/// `(evidence withdrawn, facts retracted)`.
pub async fn withdraw_evidence(
    conn: &mut PgConnection,
    version_id: &str,
    reason: &str,
) -> Result<(u64, u64), IngestError> {
    let ev = sqlx::query(
        "UPDATE _oo_docs.evidence SET withdrawn_at = clock_timestamp(), withdrawn_reason = $2 \
         WHERE version_id = $1::uuid AND withdrawn_at IS NULL",
    )
    .bind(version_id)
    .bind(reason)
    .execute(&mut *conn)
    .await?
    .rows_affected();
    let facts = sqlx::query(
        "UPDATE _oo_docs.fact f SET status = 'retracted', retracted_at = clock_timestamp() \
         WHERE f.origin <> 'human' AND f.status IN ('draft', 'published') \
         AND EXISTS (SELECT 1 FROM _oo_docs.evidence e WHERE e.target_kind = 'fact' \
                     AND e.target_id = f.id AND e.version_id = $1::uuid) \
         AND NOT EXISTS (SELECT 1 FROM _oo_docs.evidence e WHERE e.target_kind = 'fact' \
                     AND e.target_id = f.id AND e.withdrawn_at IS NULL)",
    )
    .bind(version_id)
    .execute(&mut *conn)
    .await?
    .rows_affected();
    Ok((ev, facts))
}

/// A file locked and stamped for deletion; finish with [`finish_delete`].
#[derive(Clone, Debug)]
pub struct PreparedDelete {
    /// File id.
    pub file_id: String,
    /// State at the lock.
    pub from: FileState,
    /// Version whose evidence was withdrawn.
    pub version_id: Option<String>,
}

/// First half of a delete (4.3.5): cancel the file's jobs, lock it, stamp
/// `deleted_at` on file and current version, withdraw evidence and retract
/// orphaned facts. No event yet, so a rename can prepare both files before
/// the first `append_event`. `None` when the file is gone or already deleted.
pub async fn prepare_delete(
    conn: &mut PgConnection,
    file_id: &str,
) -> Result<Option<PreparedDelete>, IngestError> {
    cancel_file_jobs(conn, file_id, None, true).await?;
    let Some((state_text, deleted)) = lock_file(conn, file_id).await? else {
        return Ok(None);
    };
    let Some(from) = FileState::parse(&state_text) else {
        return Ok(None);
    };
    if deleted {
        return Ok(None);
    }
    let vid: Option<String> = sqlx::query_scalar(
        "UPDATE _oo_docs.file SET deleted_at = clock_timestamp() WHERE id = $1::uuid \
         RETURNING current_version_id::text",
    )
    .bind(file_id)
    .fetch_one(&mut *conn)
    .await?;
    if let Some(vid) = &vid {
        sqlx::query(
            "UPDATE _oo_docs.file_version SET deleted_at = clock_timestamp() \
             WHERE id = $1::uuid AND deleted_at IS NULL",
        )
        .bind(vid)
        .execute(&mut *conn)
        .await?;
        withdraw_evidence(conn, vid, "file_deleted").await?;
    }
    Ok(Some(PreparedDelete {
        file_id: file_id.to_owned(),
        from,
        version_id: vid,
    }))
}

/// Second half: the `-> deleted` transition and its event.
pub async fn finish_delete(
    conn: &mut PgConnection,
    prepared: &PreparedDelete,
    code: Option<&str>,
    detail_json: Option<&str>,
) -> Result<(), IngestError> {
    state::transition_in_tx(
        conn,
        &Transition {
            file_id: &prepared.file_id,
            expect_from: Some(prepared.from),
            to: FileState::Deleted,
            code,
            retryable: false,
            version_id: prepared.version_id.as_deref(),
            attempt: None,
            detail_json,
        },
    )
    .await?;
    Ok(())
}

/// Delete a file in one go (caller opens the transaction).
pub async fn delete_file(
    conn: &mut PgConnection,
    file_id: &str,
    code: Option<&str>,
    detail_json: Option<&str>,
) -> Result<bool, IngestError> {
    match prepare_delete(conn, file_id).await? {
        Some(p) => {
            finish_delete(conn, &p, code, detail_json).await?;
            Ok(true)
        }
        None => Ok(false),
    }
}

/// `rename` event (4.3.2), appended by the caller after its transitions.
pub async fn append_renamed(
    conn: &mut PgConnection,
    drive_id: &str,
    new_file_id: &str,
    old_path: &str,
    new_path: &str,
) -> Result<(), IngestError> {
    let payload = serde_json::json!({
        "file_id": new_file_id,
        "old_path": old_path,
        "new_path": new_path,
    })
    .to_string();
    events::append_event(
        conn,
        "file.renamed",
        Some(drive_id),
        Some(new_file_id),
        &payload,
    )
    .await?;
    Ok(())
}

/// Hex helper re-export for callers that key maps by raw key.
pub fn key_hex(raw: &[u8]) -> String {
    hex(raw)
}

/// Whether a listed fingerprint equals the stored one. `ctime` is compared
/// only when the stored side has it (fingerprints rebuilt from the `file`
/// columns do not).
pub fn fp_matches(listed: &Fingerprint, stored: &Fingerprint) -> bool {
    listed.size == stored.size
        && listed.last_modified_ns == stored.last_modified_ns
        && listed.etag == stored.etag
        && listed.version_id == stored.version_id
        && listed.fs_ino == stored.fs_ino
        && (stored.fs_ctime_ns.is_none() || listed.fs_ctime_ns == stored.fs_ctime_ns)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fp() -> Fingerprint {
        Fingerprint {
            size: 10,
            etag: None,
            last_modified_ns: Some(5),
            version_id: None,
            fs_ino: Some(7),
            fs_ctime_ns: Some(9),
        }
    }

    #[test]
    fn any_field_change_is_a_change() {
        let stored = fp();
        assert!(fp_matches(&fp(), &stored));
        for f in [
            |x: &mut Fingerprint| x.size += 1,
            |x: &mut Fingerprint| x.last_modified_ns = Some(6),
            |x: &mut Fingerprint| x.fs_ino = Some(8),
            |x: &mut Fingerprint| x.fs_ctime_ns = Some(10),
            |x: &mut Fingerprint| x.etag = Some("e".into()),
        ] {
            let mut listed = fp();
            f(&mut listed);
            assert!(!fp_matches(&listed, &stored));
        }
    }

    #[test]
    fn ctime_is_ignored_when_the_stored_side_lacks_it() {
        let mut stored = fp();
        stored.fs_ctime_ns = None;
        let mut listed = fp();
        listed.fs_ctime_ns = Some(99);
        assert!(fp_matches(&listed, &stored));
    }
}
