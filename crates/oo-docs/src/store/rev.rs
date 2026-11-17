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

/// The largest `r` such that every file version change with `drive_rev <= r`
/// is terminal, per Drive the Drive's `rev` when nothing is pending, and the
/// minimum over Drives. `settling` are files this transaction is about to
/// publish: they count as terminal.
pub async fn covers_drive_rev(
    conn: &mut PgConnection,
    settling: &[String],
) -> Result<i64, StoreError> {
    let covers: Option<i64> = sqlx::query_scalar(
        "SELECT min(c) FROM ( \
           SELECT COALESCE( \
                    (SELECT min(fv.drive_rev) - 1 FROM _oo_docs.file f \
                       JOIN _oo_docs.file_version fv ON fv.id = f.current_version_id \
                      WHERE f.drive_id = d.id AND fv.drive_rev IS NOT NULL \
                        AND f.state <> ALL($1::text[]) AND NOT (f.id = ANY($2::text[]::uuid[]))), \
                    d.rev) AS c \
             FROM _oo_docs.drive d WHERE d.status <> 'deleted') t",
    )
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
