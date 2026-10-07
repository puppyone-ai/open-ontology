//! Loopback HTTP surface used only to prove the pinned Axum and Tower crates.
//! No authentication, no query extractor, and no product route.

#![forbid(unsafe_code)]

use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use axum::body::Bytes;
use axum::extract::State;
use axum::routing::{get, post};
use axum::Router;
use http::StatusCode;
use tokio::sync::oneshot;
use tower::limit::GlobalConcurrencyLimitLayer;
use tower_http::limit::RequestBodyLimitLayer;
use tower_http::timeout::TimeoutLayer;

/// Body cap enforced by `tower_http::limit::RequestBodyLimitLayer`.
pub const BODY_LIMIT: usize = 32;

/// Deadline applied only to `/slow`.
pub const SLOW_DEADLINE: Duration = Duration::from_millis(150);

/// How long `/slow` waits. This is longer than [`SLOW_DEADLINE`].
pub const SLOW_SLEEP: Duration = Duration::from_secs(2);

/// Shared test gate. The product Gateway does not use this type.
#[derive(Clone)]
pub struct ProbeGate {
    pub hold_rx: Arc<Mutex<Option<oneshot::Receiver<()>>>>,
    pub hold_entered: Arc<AtomicBool>,
    pub echo_started: Arc<AtomicUsize>,
}

impl ProbeGate {
    #[must_use]
    pub fn new(hold_rx: oneshot::Receiver<()>) -> Self {
        Self {
            hold_rx: Arc::new(Mutex::new(Some(hold_rx))),
            hold_entered: Arc::new(AtomicBool::new(false)),
            echo_started: Arc::new(AtomicUsize::new(0)),
        }
    }
}

// `Router` is already `#[must_use]`. A bare second attribute fails `clippy::double_must_use`.
pub fn router(gate: ProbeGate) -> Router {
    Router::new()
        .route("/slow", get(slow))
        .route_layer(TimeoutLayer::with_status_code(
            StatusCode::REQUEST_TIMEOUT,
            SLOW_DEADLINE,
        ))
        .route("/echo", post(echo))
        .route("/hold", get(hold))
        .layer(RequestBodyLimitLayer::new(BODY_LIMIT))
        // Axum applies a layer to each route separately. This layer shares one semaphore.
        .layer(GlobalConcurrencyLimitLayer::new(1))
        .with_state(gate)
}

async fn echo(State(gate): State<ProbeGate>, body: Bytes) -> Bytes {
    gate.echo_started.fetch_add(1, Ordering::SeqCst);
    body
}

async fn slow() -> &'static str {
    tokio::time::sleep(SLOW_SLEEP).await;
    "late"
}

async fn hold(State(gate): State<ProbeGate>) -> &'static str {
    let receiver = gate
        .hold_rx
        .lock()
        .expect("hold gate")
        .take()
        .expect("hold receiver installed once");
    gate.hold_entered.store(true, Ordering::SeqCst);
    let _ = receiver.await;
    "held"
}
