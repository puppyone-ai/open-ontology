//! Version retention (10 §16.9, D58).
//!
//! `ontology_version.pinned_by` (the view `ontology_version_pins`) counts the
//! live Shares that hold a version. The sweep removes only the materialised
//! caches of a version nobody pins and that was published at least 30 days
//! ago: the bytes of its compiled packs in the CAS. Facts, evidence and the
//! pack rows stay (they are append-only), so any version can still be rebuilt:
//! compiling the same request again writes the same bytes back (`persist`
//! heals a missing object before it answers). The newest version is never
//! swept, and an object that a pinned or younger pack also uses is kept.

use std::sync::Arc;
use std::time::Duration;

use sqlx::{PgConnection, PgPool};
use tokio::sync::watch;
use tokio::task::JoinHandle;

use crate::cas::{Cas, CasKind};
use crate::store::StoreError;

/// A version's caches are kept this long after its publication.
pub const RETAIN_DAYS: i64 = 30;
/// How often the engine sweeps.
pub const SWEEP_EVERY: Duration = Duration::from_secs(6 * 3600);

/// What a sweep did.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Report {
    /// Versions whose caches were swept.
    pub versions: Vec<i64>,
    /// Pack objects removed from the CAS.
    pub objects: u32,
}

/// Remove the derived pack bytes of every version with `pinned_by = 0`
/// published at least `days` days ago, except the newest version.
pub async fn sweep(conn: &mut PgConnection, cas: &Cas, days: i64) -> Result<Report, StoreError> {
    let versions: Vec<i64> = sqlx::query_scalar(
        "SELECT v.n FROM _oo_docs.ontology_version v \
           JOIN _oo_docs.ontology_version_pins pin ON pin.n = v.n \
          WHERE pin.pinned_by = 0 \
            AND v.published_at < clock_timestamp() - make_interval(days => $1::int) \
            AND v.n < (SELECT max(n) FROM _oo_docs.ontology_version) \
          ORDER BY v.n",
    )
    .bind(i32::try_from(days).unwrap_or(i32::MAX))
    .fetch_all(&mut *conn)
    .await?;
    if versions.is_empty() {
        return Ok(Report::default());
    }
    let objects: Vec<String> = sqlx::query_scalar(
        "SELECT DISTINCT f.cas_b3 FROM _oo_docs.context_pack_file f \
           JOIN _oo_docs.context_pack p ON p.id = f.pack_id \
          WHERE p.version_n = ANY($1) \
         EXCEPT \
         SELECT f.cas_b3 FROM _oo_docs.context_pack_file f \
           JOIN _oo_docs.context_pack p ON p.id = f.pack_id \
          WHERE NOT (p.version_n = ANY($1))",
    )
    .bind(&versions)
    .fetch_all(&mut *conn)
    .await?;
    let mut removed = 0;
    for b3 in &objects {
        if matches!(cas.remove(CasKind::Pack, b3), Ok(true)) {
            removed += 1;
        }
    }
    Ok(Report {
        versions,
        objects: removed,
    })
}

/// The engine's periodic sweep.
pub fn spawn(
    pool: PgPool,
    cas: Arc<Cas>,
    every: Duration,
    mut stop: watch::Receiver<bool>,
) -> JoinHandle<()> {
    tokio::spawn(async move {
        loop {
            if let Ok(mut c) = pool.acquire().await
                && let Ok(r) = sweep(&mut c, &cas, RETAIN_DAYS).await
                && r.objects > 0
            {
                crate::obs::log(
                    "info",
                    "retention.swept",
                    serde_json::json!({"versions": r.versions.len(), "objects": r.objects}),
                );
            }
            tokio::select! {
                _ = tokio::time::sleep(every) => {}
                _ = stop.changed() => return,
            }
        }
    })
}
