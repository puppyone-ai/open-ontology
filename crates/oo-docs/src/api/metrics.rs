//! `GET /v1/docs/metrics`: Prometheus text (10 §10.2, G13).
//!
//! Gauges come straight from the tables on every scrape. The stage duration
//! histograms are computed from the state history of recently published
//! files, so they survive an engine restart and cost no in-memory state. Label
//! values are fixed vocabularies (state, kind, stage names); no path, name or
//! id ever becomes a label.

use std::collections::BTreeMap;
use std::fmt::Write as _;
use std::sync::Arc;

use axum::body::Body;
use axum::http::Response;
use sqlx::PgConnection;

use super::error::ApiError;
use super::{AppState, http};
use crate::obs::latency::{self, STAGES};

/// Histogram bucket bounds in milliseconds (10 §10.2).
pub const BUCKETS_MS: [i64; 7] = [50, 200, 1_000, 5_000, 30_000, 120_000, 600_000];
/// Published files whose history feeds the histograms.
const SAMPLE_FILES: i64 = 5_000;

/// A cumulative histogram over [`BUCKETS_MS`].
#[derive(Clone, Debug, Default)]
pub struct Histogram {
    counts: [u64; 7],
    sum: i64,
    n: u64,
}

impl Histogram {
    /// Record one value.
    pub fn observe(&mut self, ms: i64) {
        for (i, b) in BUCKETS_MS.iter().enumerate() {
            if ms <= *b {
                self.counts[i] += 1;
            }
        }
        self.sum += ms;
        self.n += 1;
    }

    /// Observations so far.
    pub fn count(&self) -> u64 {
        self.n
    }

    /// Exposition lines for `name` with an optional `label="value"` pair.
    pub fn render(&self, out: &mut String, name: &str, label: Option<(&str, &str)>) {
        let lab = |extra: &str| match label {
            Some((k, v)) => format!("{{{k}=\"{v}\",{extra}}}"),
            None => format!("{{{extra}}}"),
        };
        for (i, b) in BUCKETS_MS.iter().enumerate() {
            let _ = writeln!(
                out,
                "{name}_bucket{} {}",
                lab(&format!("le=\"{b}\"")),
                self.counts[i]
            );
        }
        let _ = writeln!(out, "{name}_bucket{} {}", lab("le=\"+Inf\""), self.n);
        let plain = match label {
            Some((k, v)) => format!("{{{k}=\"{v}\"}}"),
            None => String::new(),
        };
        let _ = writeln!(out, "{name}_sum{plain} {}", self.sum);
        let _ = writeln!(out, "{name}_count{plain} {}", self.n);
    }
}

fn gauge_header(out: &mut String, name: &str, kind: &str, help: &str) {
    let _ = writeln!(out, "# HELP {name} {help}");
    let _ = writeln!(out, "# TYPE {name} {kind}");
}

/// The exposition text. Takes a connection so tests can call it directly.
pub async fn render(app: &AppState, conn: &mut PgConnection) -> Result<String, ApiError> {
    let mut out = String::new();

    gauge_header(&mut out, "oo_docs_files", "gauge", "Files by state.");
    let rows: Vec<(String, i64)> = sqlx::query_as(
        "SELECT state, count(*)::bigint FROM _oo_docs.file WHERE deleted_at IS NULL \
         GROUP BY state ORDER BY state",
    )
    .fetch_all(&mut *conn)
    .await?;
    for (state, n) in rows {
        let _ = writeln!(out, "oo_docs_files{{state=\"{state}\"}} {n}");
    }

    gauge_header(&mut out, "oo_docs_jobs", "gauge", "Jobs by kind and state.");
    let rows: Vec<(String, String, i64)> = sqlx::query_as(
        "SELECT kind, state, count(*)::bigint FROM _oo_docs.job GROUP BY kind, state ORDER BY 1, 2",
    )
    .fetch_all(&mut *conn)
    .await?;
    for (kind, state, n) in rows {
        let _ = writeln!(out, "oo_docs_jobs{{kind=\"{kind}\",state=\"{state}\"}} {n}");
    }

    gauge_header(
        &mut out,
        "oo_docs_review_pending",
        "gauge",
        "Pending review items by kind.",
    );
    let rows: Vec<(String, i64)> = sqlx::query_as(
        "SELECT kind, count(*)::bigint FROM _oo_docs.review_item WHERE status = 'pending' \
         GROUP BY kind ORDER BY kind",
    )
    .fetch_all(&mut *conn)
    .await?;
    for (kind, n) in rows {
        let _ = writeln!(out, "oo_docs_review_pending{{kind=\"{kind}\"}} {n}");
    }

    gauge_header(
        &mut out,
        "oo_docs_ontology_version",
        "gauge",
        "Latest ontology version.",
    );
    let v: Option<i64> = sqlx::query_scalar("SELECT max(n) FROM _oo_docs.ontology_version")
        .fetch_one(&mut *conn)
        .await?;
    let _ = writeln!(out, "oo_docs_ontology_version {}", v.unwrap_or(0));

    gauge_header(
        &mut out,
        "oo_docs_events_head",
        "gauge",
        "Highest event sequence.",
    );
    let head = crate::store::events::head(conn).await?;
    let _ = writeln!(out, "oo_docs_events_head {head}");

    gauge_header(
        &mut out,
        "oo_docs_sse_clients",
        "gauge",
        "Open event streams.",
    );
    let _ = writeln!(
        out,
        "oo_docs_sse_clients {}",
        super::MAX_STREAMS.saturating_sub(app.sse_slots.available_permits())
    );

    let h = app.health.snapshot().await;
    gauge_header(
        &mut out,
        "oo_docs_cas_bytes",
        "gauge",
        "Bytes in the content store by area.",
    );
    let _ = writeln!(
        out,
        "oo_docs_cas_bytes{{area=\"all\"}} {}",
        h["disk"]["cas_used_bytes"].as_u64().unwrap_or(0)
    );
    let _ = writeln!(
        out,
        "oo_docs_cas_bytes{{area=\"orig\"}} {}",
        h["disk"]["original_cache_bytes"].as_u64().unwrap_or(0)
    );
    gauge_header(
        &mut out,
        "oo_docs_disk_free_bytes",
        "gauge",
        "Free bytes where the content store lives.",
    );
    if let Some(f) = h["disk"]["free_bytes"].as_u64() {
        let _ = writeln!(out, "oo_docs_disk_free_bytes{{area=\"cas\"}} {f}");
    }

    // Per stage and end to end latency of published files (G13, D49).
    let mut stage_h: BTreeMap<&str, Histogram> =
        STAGES.iter().map(|s| (*s, Histogram::default())).collect();
    let mut total = Histogram::default();
    let mut llm = Histogram::default();
    let ids: Vec<String> = sqlx::query_scalar(
        "SELECT id::text FROM _oo_docs.file WHERE state = 'published' AND deleted_at IS NULL \
         ORDER BY state_since DESC LIMIT $1",
    )
    .bind(SAMPLE_FILES)
    .fetch_all(&mut *conn)
    .await?;
    for (_, v) in latency::for_files(conn, &ids).await? {
        for s in STAGES {
            if let Some(ms) = v[s].as_i64() {
                stage_h.entry(s).or_default().observe(ms);
            }
        }
        if let Some(ms) = v["total_ms"].as_i64() {
            total.observe(ms);
        }
        if let Some(ms) = v["llm_wait_ms"].as_i64() {
            llm.observe(ms);
        }
    }
    gauge_header(
        &mut out,
        "oo_docs_stage_duration_ms",
        "histogram",
        "Time published files spent in each stage of their latest cycle.",
    );
    for (stage, hist) in &stage_h {
        hist.render(
            &mut out,
            "oo_docs_stage_duration_ms",
            Some(("stage", stage)),
        );
    }
    gauge_header(
        &mut out,
        "oo_docs_file_latency_ms",
        "histogram",
        "Stored to published, per file, without model wait.",
    );
    total.render(&mut out, "oo_docs_file_latency_ms", None);
    gauge_header(
        &mut out,
        "oo_docs_llm_wait_ms",
        "histogram",
        "Model wait per published file.",
    );
    llm.render(&mut out, "oo_docs_llm_wait_ms", None);
    Ok(out)
}

/// `GET /v1/docs/metrics`.
pub async fn get(app: &Arc<AppState>) -> Result<Response<Body>, ApiError> {
    let mut conn = app.pool.acquire().await?;
    let text = render(app, &mut conn).await?;
    Ok(http::text(
        200,
        "text/plain; version=0.0.4; charset=utf-8",
        text,
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn histogram_is_cumulative() {
        let mut h = Histogram::default();
        for ms in [10, 60, 60, 999, 700_000] {
            h.observe(ms);
        }
        let mut out = String::new();
        h.render(&mut out, "m", Some(("stage", "parse_ms")));
        assert!(out.contains("m_bucket{stage=\"parse_ms\",le=\"50\"} 1\n"));
        assert!(out.contains("m_bucket{stage=\"parse_ms\",le=\"200\"} 3\n"));
        assert!(out.contains("m_bucket{stage=\"parse_ms\",le=\"1000\"} 4\n"));
        assert!(out.contains("m_bucket{stage=\"parse_ms\",le=\"600000\"} 4\n"));
        assert!(out.contains("m_bucket{stage=\"parse_ms\",le=\"+Inf\"} 5\n"));
        assert!(out.contains("m_sum{stage=\"parse_ms\"} 701129\n"));
        assert!(out.contains("m_count{stage=\"parse_ms\"} 5\n"));
        let mut plain = String::new();
        h.render(&mut plain, "t", None);
        assert!(plain.contains("t_bucket{le=\"50\"} 1\n") && plain.contains("t_count 5\n"));
    }
}
