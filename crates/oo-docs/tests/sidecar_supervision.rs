//! Supervision of the Docling sidecar (10 §4.4.4): a killed worker, crash
//! attribution, deadlines, busy, and the breaker around a dead sidecar.
//! `#[ignore]`: needs docker and the `oo-docs-parser:dev` image.
//!
//!     cargo test -p oo-docs --locked --test sidecar_supervision -- --ignored --nocapture --test-threads=1

mod sidecar_support;

use std::sync::atomic::Ordering;
use std::thread;
use std::time::{Duration, Instant};

use oo_core::docir::DocFormat;
use oo_docs::parse::sidecar::{Breaker, SidecarClient, SidecarError, sha256_hex};
use sidecar_support::{Sidecar, docker, fixture, golden};

fn wait_busy(c: &SidecarClient, want: u64) {
    let t0 = Instant::now();
    while t0.elapsed() < Duration::from_secs(40) {
        if c.info().is_ok_and(|i| i["busy"].as_u64() == Some(want)) {
            return;
        }
        thread::sleep(Duration::from_millis(100));
    }
    panic!("sidecar never reached busy={want}");
}

/// A parse long enough to be interrupted: 60 pages in one window.
fn start_long_parse(
    c: &SidecarClient,
    bytes: &[u8],
    deadline_ms: u64,
) -> thread::JoinHandle<Result<serde_json::Value, SidecarError>> {
    let sha = sha256_hex(bytes);
    c.put_blob(&sha, bytes).unwrap();
    let cfg = c_config(c);
    thread::spawn(move || {
        let c2 = SidecarClient::new(cfg);
        c2.parse_raw(&sha, "pdf", Some((1, 60)), deadline_ms)
    })
}

fn c_config(c: &SidecarClient) -> oo_docs::parse::sidecar::SidecarConfig {
    c.config().clone()
}

#[test]
#[ignore = "needs docker and the oo-docs-parser:dev image"]
fn sidecar_kill_worker_mid_parse_is_a_crash_and_the_service_survives() {
    let s = Sidecar::start(2);
    let c = s.client();
    let long = fixture("pdf-60pages.pdf");
    let h = start_long_parse(&c, &long, 110_000);
    wait_busy(&c, 1);
    let killed = s.kill_workers_soon(Duration::from_secs(30));
    assert!(killed >= 1, "no worker found to kill");
    let err = h.join().unwrap().unwrap_err();
    assert!(
        matches!(&err, SidecarError::Crash(m) if m.contains("signal 9")),
        "{err:?}"
    );
    assert_eq!(err.code(), Some("DOCS_PARSE_CRASH"));
    assert!(err.consumes_attempt() && !err.is_infrastructure());
    // The HTTP process kept serving; the crash is counted by the sidecar.
    let info = c.info().unwrap();
    assert_eq!(info["stats"]["crash"], 1);
    assert_eq!(info["busy"], 0);
    // And a normal document still parses afterwards.
    let n = c
        .parse_bytes(
            &golden("docs/Proposals/灯塔迁移方案.pdf"),
            DocFormat::Pdf,
            &mut |_| {},
        )
        .unwrap();
    assert!(!n.ir.blocks.is_empty());
    assert!(
        !c.breaker_open(),
        "a per-document crash must not open the breaker"
    );
}

#[test]
#[ignore = "needs docker and the oo-docs-parser:dev image"]
fn sidecar_crash_count_accumulates_on_the_client() {
    let s = Sidecar::start(2);
    let c = s.client();
    let long = fixture("pdf-60pages.pdf");
    let sha = sha256_hex(&long);
    c.put_blob(&sha, &long).unwrap();
    for round in 1..=2u64 {
        let cfg = s.config();
        let sha2 = sha.clone();
        let h = thread::spawn(move || {
            SidecarClient::new(cfg).parse_raw(&sha2, "pdf", Some((1, 60)), 110_000)
        });
        wait_busy(&c, 1);
        assert!(s.kill_workers_soon(Duration::from_secs(30)) >= 1);
        assert!(
            matches!(h.join().unwrap(), Err(SidecarError::Crash(_))),
            "round {round}"
        );
        // Count what this client saw through its own call path.
        let own = c.parse_raw(&sha, "pdf", Some((1, 1)), 110_000);
        assert!(own.is_ok(), "{own:?}");
    }
    assert_eq!(c.info().unwrap()["stats"]["crash"], 2);
}

#[test]
#[ignore = "needs docker and the oo-docs-parser:dev image"]
fn sidecar_deadline_kills_the_worker_with_a_timeout() {
    let s = Sidecar::start(2);
    let c = s.client();
    let long = fixture("pdf-60pages.pdf");
    let sha = sha256_hex(&long);
    c.put_blob(&sha, &long).unwrap();
    let t0 = Instant::now();
    let err = c.parse_raw(&sha, "pdf", Some((1, 60)), 1_000).unwrap_err();
    assert_eq!(err, SidecarError::Timeout);
    assert_eq!(err.code(), Some("DOCS_PARSE_TIMEOUT"));
    assert!(
        t0.elapsed() < Duration::from_secs(20),
        "deadline was not enforced: {:?}",
        t0.elapsed()
    );
    assert_eq!(c.stats.timeouts.load(Ordering::Relaxed), 1);
    let info = c.info().unwrap();
    assert_eq!(info["stats"]["timeout"], 1);
    assert_eq!(info["busy"], 0, "the killed worker must free its slot");
}

#[test]
#[ignore = "needs docker and the oo-docs-parser:dev image"]
fn sidecar_busy_is_429_without_consuming_an_attempt() {
    let s = Sidecar::start(2);
    let c = s.client();
    let long = fixture("pdf-60pages.pdf");
    let h1 = start_long_parse(&c, &long, 25_000);
    let h2 = start_long_parse(&c, &long, 25_000);
    wait_busy(&c, 2);
    let sha = sha256_hex(&long);
    let err = c.parse_raw(&sha, "pdf", Some((1, 1)), 10_000).unwrap_err();
    assert!(
        matches!(
            err,
            SidecarError::Busy {
                retry_after_ms: 2000
            }
        ),
        "{err:?}"
    );
    assert!(!err.consumes_attempt());
    assert_eq!(err.code(), None);
    assert!(!c.breaker_open(), "busy is not an infrastructure error");
    assert_eq!(c.stats.busy.load(Ordering::Relaxed), 1);
    // Both running requests end by their own deadline (or finish).
    for h in [h1, h2] {
        let r = h.join().unwrap();
        assert!(
            r.is_ok() || matches!(r, Err(SidecarError::Timeout)),
            "{r:?}"
        );
    }
    assert_eq!(c.info().unwrap()["busy"], 0);
}

#[test]
#[ignore = "needs docker and the oo-docs-parser:dev image"]
fn sidecar_dead_sidecar_opens_the_breaker_and_a_restart_closes_it() {
    let s = Sidecar::start(2);
    let name = s.name.clone().expect("owned container");
    let c = SidecarClient::with_breaker(s.config(), Breaker::new(3, Duration::from_secs(2)));
    assert!(c.info().is_ok());
    let (ok, out) = docker(&["stop", "-t", "2", &name]);
    assert!(ok, "{out}");
    for _ in 0..3 {
        assert!(matches!(c.info(), Err(SidecarError::Unavailable(_))));
    }
    assert!(c.breaker_open());
    assert!(matches!(c.info(), Err(SidecarError::CircuitOpen { .. })));
    assert!(!SidecarError::CircuitOpen { retry_after_ms: 1 }.consumes_attempt());
    // The daemon's reaction: restart the container (published port may change).
    let (ok, out) = docker(&["start", &name]);
    assert!(ok, "{out}");
    let (_, port) = docker(&["port", &name, "7481/tcp"]);
    let port = port
        .lines()
        .next()
        .and_then(|l| l.rsplit(':').next())
        .unwrap()
        .to_owned();
    let mut cfg = s.config();
    cfg.addr = format!("127.0.0.1:{port}");
    let healthy = SidecarClient::new(cfg.clone());
    let t0 = Instant::now();
    while !healthy.healthy() && t0.elapsed() < Duration::from_secs(60) {
        thread::sleep(Duration::from_millis(300));
    }
    assert!(healthy.healthy());
    // Same breaker, new address: after the open window one probe closes it.
    let c2 = SidecarClient::with_breaker(cfg, Breaker::new(3, Duration::from_secs(1)));
    assert!(c2.info().is_ok());
    assert!(!c2.breaker_open());
}

#[test]
#[ignore = "needs docker and the oo-docs-parser:dev image"]
fn sidecar_rejects_a_wrong_token_and_hash_mismatches() {
    let s = Sidecar::start(2);
    let mut cfg = s.config();
    cfg.token = "not-the-token".into();
    let bad = SidecarClient::new(cfg);
    assert!(matches!(bad.info(), Err(SidecarError::Protocol(_))));
    let c = s.client();
    let err = c
        .put_blob(&"0".repeat(64), b"does not hash to that")
        .unwrap_err();
    assert_eq!(err, SidecarError::HashMismatch);
    assert!(c.put_blob(&sha256_hex(b"x"), b"x").is_ok());
    assert_eq!(c.put_blob(&sha256_hex(b"x"), b"x"), Ok(true));
}
