//! Freshness token `drive_rev` and the coverage it allows (10 §16.2, FM10).
//!
//! `drive.rev` advances in the same transaction as every new file version, as
//! every file that becomes `superseded` or `deleted` (database triggers) and
//! as every correction overlay ([`bump_drive_rev`]). A published version
//! covers `r` when every change with `drive_rev <= r` has reached a terminal
//! state: [`covers_drive_rev`] is the largest such `r`, taken over all Drives
//! (the smallest per-Drive value, so the number is safe for any of them).

use sqlx::PgConnection;

use super::StoreError;

/// File states in which a change is settled: nothing is left to publish for it.
pub const TERMINAL_STATES: &[&str] = &[
    "published",
    "parse_failed",
    "extract_partial",
    "unsupported",
    "too_large",
    "encrypted",
    "duplicate",
    "superseded",
    "deleted",
    "outdated",
];

/// Advance a Drive's `rev` and return the new value.
pub async fn bump_drive_rev(conn: &mut PgConnection, drive_id: &str) -> Result<i64, StoreError> {
    let rev: i64 = sqlx::query_scalar("SELECT _oo_docs.bump_drive_rev($1::uuid)")
        .bind(drive_id)
        .fetch_one(&mut *conn)
        .await?;
    Ok(rev)
}

/// The largest `r` such that every change with `drive_rev <= r` is settled,
/// per Drive the Drive's `rev` when nothing is pending, and the minimum over
/// Drives. `settling` are files this transaction is about to publish: they
/// count as terminal. A correction (overlay) is settled once a publish run that
/// started after it was written has finished (16.2: a correction takes a
/// `drive_rev` like a file change and is not covered before a version holds
/// it): at publish time that is "written before this transaction started".
pub async fn covers_drive_rev(
    conn: &mut PgConnection,
    settling: &[String],
) -> Result<i64, StoreError> {
    covers(conn, settling, "transaction_timestamp()").await
}

/// [`covers_drive_rev`] as measured now, between publishes: a correction is
/// pending until a version was published after it was written.
pub async fn covers_drive_rev_live(conn: &mut PgConnection) -> Result<i64, StoreError> {
    covers(
        conn,
        &[],
        "COALESCE((SELECT max(v.published_at) FROM _oo_docs.ontology_version v), '-infinity'::timestamptz)",
    )
    .await
}

/// [`covers_drive_rev_live`] for one Drive and the files of `prefixes` only (empty: the whole Drive, but still this
/// Drive alone). A pack of a scope waits for the files of its scope, not for a long recording elsewhere in the Drive
/// (D77): a file outside the scope that is still being parsed does not hold the freshness floor of a share or a
/// compile that cannot contain it. Corrections (overlays) are Drive wide, as before.
pub async fn covers_drive_rev_scoped(
    conn: &mut PgConnection,
    drive_id: &str,
    prefixes: &[String],
) -> Result<i64, StoreError> {
    let sql = "SELECT LEAST( \
                 COALESCE( \
                   (SELECT min(fv.drive_rev) - 1 FROM _oo_docs.file f \
                      JOIN _oo_docs.file_version fv ON fv.id = f.current_version_id \
                     WHERE f.drive_id = d.id AND fv.drive_rev IS NOT NULL \
                       AND f.state <> ALL($2::text[]) \
                       AND (cardinality($3::text[]) = 0 OR EXISTS (SELECT 1 FROM unnest($3::text[]) sp(p) \
                             WHERE f.path = sp.p OR f.path LIKE replace(replace(replace(sp.p, '\\', '\\\\'), '%', '\\%'), '_', '\\_') || '/%'))), \
                   d.rev), \
                 COALESCE( \
                   (SELECT min(o.drive_rev) - 1 FROM _oo_docs.overlay o \
                     WHERE o.drive_rev IS NOT NULL \
                       AND o.created_at > COALESCE((SELECT max(v.published_at) FROM _oo_docs.ontology_version v), '-infinity'::timestamptz)), \
                   d.rev)) \
               FROM _oo_docs.drive d WHERE d.id = $1::uuid";
    let covers: Option<i64> = sqlx::query_scalar(sql)
        .bind(drive_id)
        .bind(TERMINAL_STATES)
        .bind(prefixes)
        .fetch_optional(&mut *conn)
        .await?;
    Ok(covers.unwrap_or(0).max(0))
}

async fn covers(
    conn: &mut PgConnection,
    settling: &[String],
    overlay_cutoff: &str,
) -> Result<i64, StoreError> {
    let sql = format!(
        "SELECT min(c) FROM ( \
           SELECT LEAST( \
                    COALESCE( \
                      (SELECT min(fv.drive_rev) - 1 FROM _oo_docs.file f \
                         JOIN _oo_docs.file_version fv ON fv.id = f.current_version_id \
                        WHERE f.drive_id = d.id AND fv.drive_rev IS NOT NULL \
                          AND f.state <> ALL($1::text[]) AND NOT (f.id = ANY($2::text[]::uuid[]))), \
                      d.rev), \
                    COALESCE( \
                      (SELECT min(o.drive_rev) - 1 FROM _oo_docs.overlay o \
                        WHERE o.drive_rev IS NOT NULL AND o.created_at > {overlay_cutoff}), \
                      d.rev)) AS c \
             FROM _oo_docs.drive d WHERE d.status <> 'deleted') t"
    );
    let covers: Option<i64> = sqlx::query_scalar(sqlx::AssertSqlSafe(sql))
        .bind(TERMINAL_STATES)
        .bind(settling)
        .fetch_one(&mut *conn)
        .await?;
    Ok(covers.unwrap_or(0).max(0))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn terminal_states_are_the_settled_ones() {
        assert!(TERMINAL_STATES.contains(&"published"));
        assert!(TERMINAL_STATES.contains(&"parse_failed"));
        assert!(!TERMINAL_STATES.contains(&"indexed"));
        assert!(!TERMINAL_STATES.contains(&"queued"));
    }
}
