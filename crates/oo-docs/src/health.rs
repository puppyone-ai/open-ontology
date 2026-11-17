//! `GET /v1/docs/health` (10 §8.3, §4.12). Board WS-BE5.
//!
//! The engine always answers: when the database is unreachable the answer is
//! `status = unavailable` with `components.db.status = unavailable`, not an
//! error, so a client can tell "engine up, database down" from "engine down".

use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::time::{Duration, Instant};

use serde_json::{Value, json};
use sqlx::PgPool;

use crate::api::freshness;
use crate::store::events;

/// Free space below which hashing and parsing stop (2.8 `disk.pause_below_bytes`).
pub const DISK_LOW_BYTES: u64 = 2 * 1024 * 1024 * 1024;
/// Free space below which writes are refused (`disk.readonly_below_bytes`).
pub const DISK_FULL_BYTES: u64 = 512 * 1024 * 1024;

/// Free bytes of the filesystem holding a path. Injected so tests can pick the
/// number; `DfProbe` asks `df`, because the workspace forbids `unsafe` and has
/// no `libc` for `statvfs`.
pub trait DiskProbe: Send + Sync {
    /// Available bytes, `None` when unknown.
    fn free_bytes(&self, path: &Path) -> Option<u64>;
}

/// `df -Pk <path>`.
#[derive(Clone, Copy, Debug, Default)]
pub struct DfProbe;

impl DiskProbe for DfProbe {
    fn free_bytes(&self, path: &Path) -> Option<u64> {
        let out = std::process::Command::new("df")
            .arg("-Pk")
            .arg(path)
            .output()
            .ok()?;
        if !out.status.success() {
            return None;
        }
        let text = String::from_utf8(out.stdout).ok()?;
        let line = text.lines().nth(1)?;
        let avail: u64 = line.split_whitespace().nth(3)?.parse().ok()?;
        avail.checked_mul(1024)
    }
}

/// Everything the health answer needs besides the database.
pub struct HealthProbe {
    pool: PgPool,
    cas_root: Option<PathBuf>,
    disk: Box<dyn DiskProbe>,
    schema_versions: Vec<String>,
    started: Instant,
    ready: std::sync::atomic::AtomicBool,
    usage: Mutex<Option<(Instant, u64, u64)>>,
}

impl HealthProbe {
    /// New probe. `ready` starts false: the engine reports `starting` until
    /// `set_ready`.
    pub fn new(
        pool: PgPool,
        cas_root: Option<PathBuf>,
        disk: Box<dyn DiskProbe>,
        schema_versions: Vec<String>,
    ) -> Self {
        Self {
            pool,
            cas_root,
            disk,
            schema_versions,
            started: Instant::now(),
            ready: std::sync::atomic::AtomicBool::new(false),
            usage: Mutex::new(None),
        }
    }

    /// The engine finished starting.
    pub fn set_ready(&self) {
        self.ready.store(true, std::sync::atomic::Ordering::Relaxed);
    }

    /// Seconds since the probe was built.
    pub fn uptime(&self) -> Duration {
        self.started.elapsed()
    }

    /// Bytes under the CAS root and under `orig/`, cached for 30 s.
    fn cas_usage(&self) -> (u64, u64) {
        let Some(root) = &self.cas_root else {
            return (0, 0);
        };
        let mut cache = self.usage.lock().unwrap_or_else(|p| p.into_inner());
        if let Some((at, total, orig)) = *cache
            && at.elapsed() < Duration::from_secs(30)
        {
            return (total, orig);
        }
        let total = dir_size(root, 0);
        let orig = dir_size(&root.join("orig"), 0);
        *cache = Some((Instant::now(), total, orig));
        (total, orig)
    }

    /// The `Health` object.
    pub async fn snapshot(&self) -> Value {
        let ready = self.ready.load(std::sync::atomic::Ordering::Relaxed);
        let t0 = Instant::now();
        let mut conn = self.pool.acquire().await.ok();
        let db_ok = match conn.as_mut() {
            Some(c) => sqlx::query("SELECT 1").execute(&mut **c).await.is_ok(),
            None => false,
        };
        let latency = t0.elapsed().as_millis() as i64;
        let db = if db_ok {
            json!({ "status": "ok", "latency_ms": latency })
        } else {
            json!({ "status": "unavailable", "code": "DOCS_ENGINE_DB_UNAVAILABLE" })
        };

        // Disk and CAS.
        let free = self.cas_root.as_ref().and_then(|r| self.disk.free_bytes(r));
        let (used, orig) = self.cas_usage();
        let cas = match (&self.cas_root, free) {
            (None, _) => json!({ "status": "not_configured" }),
            (Some(_), Some(f)) if f < DISK_FULL_BYTES => {
                json!({ "status": "degraded", "code": "DOCS_ENGINE_DISK_FULL" })
            }
            (Some(_), Some(f)) if f < DISK_LOW_BYTES => {
                json!({ "status": "degraded", "code": "DOCS_ENGINE_DISK_LOW" })
            }
            _ => json!({ "status": "ok" }),
        };
        let mut disk = json!({ "cas_used_bytes": used, "original_cache_bytes": orig });
        if let Some(f) = free {
            disk["free_bytes"] = json!(f);
        }

        let (queue, head, fresh) = match (db_ok, conn.as_mut()) {
            (true, Some(c)) => {
                let queue = queue_counts(c).await.unwrap_or_else(|| json!({}));
                let head = events::head(c).await.unwrap_or(0);
                let fresh = overall_freshness(c).await;
                (queue, head, fresh)
            }
            _ => (json!({}), 0, "up_to_date"),
        };

        let component_states = [&db, &cas];
        let degraded = component_states
            .iter()
            .any(|c| matches!(c["status"].as_str(), Some("degraded" | "unavailable")));
        let status = if !ready {
            "starting"
        } else if !db_ok {
            "unavailable"
        } else if degraded {
            "degraded"
        } else {
            "ok"
        };
        json!({
            "api_version": crate::API_VERSION.trim_start_matches("docs-api/"),
            "engine_version": crate::ENGINE_VERSION,
            "schema_versions": self.schema_versions,
            "status": status,
            "components": {
                "db": db,
                // md and txt parse in a local child process; no sidecar yet.
                "parser": { "status": "ok", "detail": { "kind": "local", "formats": ["md", "txt"] } },
                "model": { "status": "not_configured" },
                "cas": cas,
                "engine": { "status": if ready { "ok" } else { "starting" } },
            },
            "queue": queue,
            "disk": disk,
            "events_head": head,
            "freshness": fresh,
        })
    }
}

async fn queue_counts(conn: &mut sqlx::PgConnection) -> Option<Value> {
    let row: (i64, i64, i64, i64, i64) = sqlx::query_as(
        "SELECT count(*) FILTER (WHERE state = 'queued'), \
                count(*) FILTER (WHERE state = 'running'), \
                count(*) FILTER (WHERE state = 'failed'), \
                count(*) FILTER (WHERE state = 'quarantined'), \
                count(*) FILTER (WHERE state = 'queued' AND blocked_reason IS NOT NULL) \
         FROM _oo_docs.job",
    )
    .fetch_one(&mut *conn)
    .await
    .ok()?;
    Some(json!({
        "queued": row.0, "running": row.1, "failed_manual": row.2,
        "quarantined": row.3, "blocked": row.4,
    }))
}

async fn overall_freshness(conn: &mut sqlx::PgConnection) -> &'static str {
    let ids: Vec<String> = sqlx::query_scalar(
        "SELECT id::text FROM _oo_docs.drive WHERE status IN ('active', 'paused')",
    )
    .fetch_all(&mut *conn)
    .await
    .unwrap_or_default();
    match freshness::compute(conn, &ids).await {
        Ok(map) => freshness::highest(map.values().filter_map(|v| v["state"].as_str())),
        Err(_) => "up_to_date",
    }
}

fn dir_size(path: &Path, depth: usize) -> u64 {
    if depth > 8 {
        return 0;
    }
    let Ok(rd) = std::fs::read_dir(path) else {
        return 0;
    };
    rd.flatten()
        .map(|e| match e.metadata() {
            Ok(m) if m.is_dir() => dir_size(&e.path(), depth + 1),
            Ok(m) => m.len(),
            Err(_) => 0,
        })
        .sum()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dir_size_sums_files() {
        let dir = std::env::temp_dir().join(format!("oo-docs-health-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(dir.join("a/b")).unwrap();
        std::fs::write(dir.join("x"), [0u8; 10]).unwrap();
        std::fs::write(dir.join("a/b/y"), [0u8; 5]).unwrap();
        assert_eq!(dir_size(&dir, 0), 15);
        assert_eq!(dir_size(&dir.join("missing"), 0), 0);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn thresholds_are_the_spec_values() {
        assert_eq!(DISK_LOW_BYTES, 2_147_483_648);
        assert_eq!(DISK_FULL_BYTES, 536_870_912);
    }

    #[test]
    fn df_probe_reads_something_for_tmp() {
        // `df` exists on the CI images and on macOS; unknown is also valid.
        if let Some(n) = DfProbe.free_bytes(&std::env::temp_dir()) {
            assert!(n > 0);
        }
    }
}
