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
use crate::cas::{Cas, gc::SampleReport};
use crate::store::events;

/// Objects hashed per sample and the time between samples.
const CAS_SAMPLE_SIZE: usize = 20;
const CAS_SAMPLE_EVERY: Duration = Duration::from_secs(300);

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

impl<T: DiskProbe + ?Sized> DiskProbe for std::sync::Arc<T> {
    fn free_bytes(&self, path: &Path) -> Option<u64> {
        (**self).free_bytes(path)
    }
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

/// How long a sidecar's `/healthz` answer is reused.
const SIDECAR_CACHE: Duration = Duration::from_secs(2);

/// One sidecar the engine uses, asked on `GET /healthz`.
type SidecarCheck = std::sync::Arc<dyn Fn() -> bool + Send + Sync>;

struct SidecarProbe {
    /// The component name in `Health.components`.
    component: &'static str,
    check: SidecarCheck,
    last: Option<(Instant, bool)>,
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
    sample: Mutex<Option<(Instant, SampleReport)>>,
    /// What the parse stage can read (`set_parser`); local md, txt, csv and xlsx by default.
    parser: Mutex<Value>,
    /// The sidecars that are configured (`doc-parser`, `ocr-parser`, `media-parser`); empty when none is.
    sidecars: tokio::sync::Mutex<Vec<SidecarProbe>>,
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
            parser: Mutex::new(serde_json::json!({
                "kind": "local", "formats": ["md", "txt", "csv", "xlsx"]
            })),
            sidecars: tokio::sync::Mutex::new(Vec::new()),
            ready: std::sync::atomic::AtomicBool::new(false),
            usage: Mutex::new(None),
            sample: Mutex::new(None),
        }
    }

    /// The engine finished starting.
    /// Say how files are parsed: `local` (md, txt, csv, xlsx in the engine) or
    /// `local+sidecar` with the Docling sidecar for pdf, docx and pptx.
    pub fn set_parser(&self, with_sidecar: bool) {
        let v = if with_sidecar {
            serde_json::json!({"kind": "local+sidecar",
                "formats": ["md", "txt", "csv", "xlsx", "pdf", "docx", "pptx"],
                "sidecar_formats": ["pdf", "docx", "pptx"]})
        } else {
            serde_json::json!({"kind": "local", "formats": ["md", "txt", "csv", "xlsx"]})
        };
        if let Ok(mut g) = self.parser.lock() {
            *g = v;
        }
    }

    /// Watch a configured sidecar: `component` is `parser` (doc-parser), `ocr_parser` or `media_parser`;
    /// `check` is its `/healthz`. `GET /health` reports `unavailable` (and the engine `degraded`) while it fails.
    pub async fn watch_sidecar(
        &self,
        component: &'static str,
        check: impl Fn() -> bool + Send + Sync + 'static,
    ) {
        self.sidecars.lock().await.push(SidecarProbe {
            component,
            check: std::sync::Arc::new(check),
            last: None,
        });
    }

    /// Component objects of the watched sidecars, probing the stale ones in parallel.
    async fn sidecar_components(&self) -> Vec<(&'static str, bool)> {
        let mut probes = self.sidecars.lock().await;
        let mut tasks = Vec::new();
        for (i, p) in probes.iter().enumerate() {
            if p.last.is_some_and(|(at, _)| at.elapsed() < SIDECAR_CACHE) {
                continue;
            }
            let check = p.check.clone();
            tasks.push((i, tokio::task::spawn_blocking(move || check())));
        }
        for (i, t) in tasks {
            // A probe that panicked counts as down.
            probes[i].last = Some((Instant::now(), t.await.unwrap_or(false)));
        }
        probes
            .iter()
            .map(|p| (p.component, p.last.is_some_and(|(_, ok)| ok)))
            .collect()
    }

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

    /// Hash a few objects of the content store, at most every five minutes
    /// (`oo doctor` `cas_sample`, 10 §10.4).
    fn cas_sample(&self) -> SampleReport {
        let Some(root) = &self.cas_root else {
            return SampleReport::default();
        };
        let mut cache = self.sample.lock().unwrap_or_else(|p| p.into_inner());
        if let Some((at, r)) = *cache
            && at.elapsed() < CAS_SAMPLE_EVERY
        {
            return r;
        }
        let seed = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_or(0, |d| d.as_secs());
        let report = Cas::open(root).map_or_else(
            |_| SampleReport::default(),
            |cas| cas.sample_verify(CAS_SAMPLE_SIZE, seed),
        );
        *cache = Some((Instant::now(), report));
        report
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
        let sample = self.cas_sample();
        let cas = match (&self.cas_root, free) {
            (None, _) => json!({ "status": "not_configured" }),
            (Some(_), Some(f)) if f < DISK_FULL_BYTES => {
                json!({ "status": "degraded", "code": "DOCS_ENGINE_DISK_FULL" })
            }
            (Some(_), Some(f)) if f < DISK_LOW_BYTES => {
                json!({ "status": "degraded", "code": "DOCS_ENGINE_DISK_LOW" })
            }
            // Damage found by the sample is reported even with space to spare.
            _ if sample.corrupt > 0 => {
                json!({ "status": "degraded", "code": "DOCS_ENGINE_CAS_CORRUPT" })
            }
            _ => json!({ "status": "ok" }),
        };
        let mut disk = json!({
            "cas_used_bytes": used,
            "original_cache_bytes": orig,
            "cas_sample": { "checked": sample.checked, "corrupt": sample.corrupt },
        });
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

        // A sidecar that is down leaves the formats it serves queued (10 §4.4.4); the rest keeps working.
        let sidecars = self.sidecar_components().await;
        let sidecar_down = |name: &str| sidecars.iter().any(|(n, ok)| *n == name && !*ok);
        let sidecar_component = |name: &str| -> Option<Value> {
            sidecars.iter().find(|(n, _)| *n == name).map(|(_, ok)| {
                if *ok {
                    json!({ "status": "ok" })
                } else {
                    json!({ "status": "unavailable", "code": "DOCS_ENGINE_PARSER_UNAVAILABLE" })
                }
            })
        };
        let mut parser = json!({ "status": "ok", "detail": self.parser.lock().map_or(Value::Null, |g| g.clone()) });
        if let Some(c) = sidecar_component("parser") {
            parser["status"] = c["status"].clone();
            if let Some(code) = c.get("code") {
                parser["code"] = code.clone();
            }
        }
        let degraded = [&db, &cas, &parser]
            .iter()
            .any(|c| matches!(c["status"].as_str(), Some("degraded" | "unavailable")))
            || sidecar_down("ocr_parser")
            || sidecar_down("media_parser");
        let status = if !ready {
            "starting"
        } else if !db_ok {
            "unavailable"
        } else if degraded {
            "degraded"
        } else {
            "ok"
        };
        let mut components = json!({
            "db": db,
            "parser": parser,
            "model": { "status": "not_configured" },
            "cas": cas,
            "engine": { "status": if ready { "ok" } else { "starting" } },
        });
        for name in ["ocr_parser", "media_parser"] {
            if let Some(c) = sidecar_component(name) {
                components[name] = c;
            }
        }
        json!({
            "api_version": crate::API_VERSION.trim_start_matches("docs-api/"),
            "engine_version": crate::ENGINE_VERSION,
            "schema_versions": self.schema_versions,
            "status": status,
            "components": components,
            "queue": queue,
            "disk": disk,
            "events_head": head,
            "freshness": fresh,
        })
    }
}

async fn queue_counts(conn: &mut sqlx::PgConnection) -> Option<Value> {
    let row: (i64, i64, i64, i64, i64, i64) = sqlx::query_as(
        "SELECT count(*) FILTER (WHERE state = 'queued'), \
                count(*) FILTER (WHERE state = 'running'), \
                count(*) FILTER (WHERE state = 'failed'), \
                count(*) FILTER (WHERE state = 'quarantined'), \
                count(*) FILTER (WHERE state = 'queued' AND blocked_reason IS NOT NULL), \
                count(*) FILTER (WHERE state = 'running' \
                                   AND lease_until < clock_timestamp() - interval '5 minutes') \
         FROM _oo_docs.job",
    )
    .fetch_one(&mut *conn)
    .await
    .ok()?;
    Some(json!({
        "queued": row.0, "running": row.1, "failed_manual": row.2,
        "quarantined": row.3, "blocked": row.4, "stuck": row.5,
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
