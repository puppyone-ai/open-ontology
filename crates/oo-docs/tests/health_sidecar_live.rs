//! `GET /v1/docs/health` follows the real sidecar (10 §4.4.4, docs-api 1.10): killing the doc-parser container turns
//! `components.parser` to `unavailable` and the engine to `degraded`; starting it again brings it back.
//! `#[ignore]`: needs a Postgres (`OO_DOCS_TEST_PG_PORT`), docker and the `oo-docs-parser:dev` image.
//!
//!     cargo test -p oo-docs --locked --test health_sidecar_live -- --ignored --nocapture --test-threads=1

mod common;
mod sidecar_support;

use std::time::{Duration, Instant};

use common::engine::{Opts, TestEngine};
use serde_json::Value;
use sidecar_support::Sidecar;

async fn health(e: &TestEngine) -> Value {
    let r = e.get("/v1/docs/health").await;
    assert_eq!(r.status, 200);
    r.json()
}

async fn until_parser(e: &TestEngine, want: &str) -> Value {
    let t0 = Instant::now();
    loop {
        let h = health(e).await;
        if h["components"]["parser"]["status"] == want {
            return h;
        }
        assert!(
            t0.elapsed() < Duration::from_secs(60),
            "parser never became {want}: {}",
            h["components"]["parser"]
        );
        tokio::time::sleep(Duration::from_millis(500)).await;
    }
}

#[tokio::test]
#[ignore = "needs OO_DOCS_TEST_PG_PORT, docker and the oo-docs-parser:dev image"]
async fn health_says_unavailable_while_the_parser_sidecar_is_dead() {
    let s = Sidecar::start(1);
    s.wait_healthy(Duration::from_secs(90));
    let e = TestEngine::start("health_sidecar", Opts::default()).await;

    // Not configured: the parse stage is the local one and is ok (md, txt, csv, xlsx).
    let h = health(&e).await;
    assert_eq!(h["components"]["parser"]["status"], "ok");
    assert!(h["components"].get("ocr_parser").is_none());

    let client = std::sync::Arc::new(s.client());
    {
        let c = client.clone();
        e.app
            .health
            .watch_sidecar("parser", move || c.healthy())
            .await;
    }
    let h = until_parser(&e, "ok").await;
    assert_eq!(h["status"], "ok");

    s.kill();
    let h = until_parser(&e, "unavailable").await;
    assert_eq!(h["status"], "degraded");
    assert_eq!(
        h["components"]["parser"]["code"],
        "DOCS_ENGINE_PARSER_UNAVAILABLE"
    );
    // The rest of the engine is not touched.
    assert_eq!(h["components"]["db"]["status"], "ok");
    assert_eq!(h["components"]["cas"]["status"], "ok");

    s.start_again();
    s.wait_healthy(Duration::from_secs(90));
    let h = until_parser(&e, "ok").await;
    assert_eq!(h["status"], "ok");
    assert!(h["components"]["parser"].get("code").is_none());
    e.finish().await;
}
