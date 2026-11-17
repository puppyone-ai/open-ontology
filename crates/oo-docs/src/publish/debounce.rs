//! Debounced publish scheduling (10 §4.9): a transaction that makes something
//! publishable (a review decision, a correction) asks for a `publish` job in
//! the current 2 second window. Everything asked for within a window shares
//! one job, so an accept reaches a new version within the window plus one
//! publish run (L4 acceptance 6: 5 seconds).

use sqlx::PgConnection;

use crate::jobs::stages::PUBLISH_WINDOW_MS;
use crate::jobs::{JobKind, JobStatus, NewJob};
use crate::store::StoreError;
use crate::store::jobs as store;

/// Start of the window that contains `now_ms`.
pub fn window_start(now_ms: i64) -> i64 {
    now_ms - now_ms.rem_euclid(PUBLISH_WINDOW_MS)
}

/// Ask for a publish. Returns the milliseconds until the job is due, or
/// `None` when a job for this window is already waiting (then it will pick the
/// change up). A window whose job already ran or failed moves on to the next
/// window, so a change is never left behind.
pub async fn schedule_publish(conn: &mut PgConnection) -> Result<Option<i64>, StoreError> {
    let now_ms: i64 =
        sqlx::query_scalar("SELECT (extract(epoch FROM clock_timestamp()) * 1000)::bigint")
            .fetch_one(&mut *conn)
            .await?;
    let start = window_start(now_ms);
    for step in 0..2 {
        let at = start + step * PUBLISH_WINDOW_MS;
        let key = format!("publish:{at}");
        let mut job = NewJob::new(JobKind::Publish, &key);
        job.run_after_ms = (at + PUBLISH_WINDOW_MS - now_ms).max(0);
        match store::enqueue(&mut *conn, &job).await?.status {
            JobStatus::Enqueued => return Ok(Some(job.run_after_ms)),
            JobStatus::Replayed => return Ok(None),
            _ => {}
        }
    }
    Ok(None)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn windows_are_two_seconds() {
        assert_eq!(PUBLISH_WINDOW_MS, 2_000);
        assert_eq!(window_start(0), 0);
        assert_eq!(window_start(1_999), 0);
        assert_eq!(window_start(2_000), 2_000);
        assert_eq!(window_start(123_457), 122_000);
    }
}
