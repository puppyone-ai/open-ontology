//! Request-owned state; dropping HTTP work must not leave a reusable busy session.
use std::sync::atomic::{AtomicU8, Ordering};
use std::sync::Arc;
use std::time::Duration;

use tokio::sync::{watch, OwnedMutexGuard};

use crate::query_executor::DedicatedSession;

pub(super) struct QueryState {
    status: AtomicU8,
    stop: watch::Sender<bool>,
}

impl QueryState {
    pub fn new() -> Arc<Self> {
        Arc::new(Self {
            status: AtomicU8::new(0),
            stop: watch::channel(false).0,
        })
    }

    pub fn status(&self) -> &'static str {
        match self.status.load(Ordering::Acquire) {
            0 => "running",
            1 => "succeeded",
            2 => "timed_out",
            3 => "canceled",
            4 => "abandoned",
            _ => "failed",
        }
    }

    pub fn finish(&self, status: &'static str) {
        let value = match status {
            "succeeded" => 1,
            "timed_out" => 2,
            "canceled" => 3,
            "abandoned" => 4,
            _ => 5,
        };
        let _ = self.status.compare_exchange(0, value, Ordering::AcqRel, Ordering::Acquire);
    }

    pub fn cancel(&self) -> &'static str {
        if self.status() != "running" {
            return "not_running";
        }
        self.stop.send_replace(true);
        "requested"
    }

    pub async fn canceled(&self) {
        let mut stop = self.stop.subscribe();
        let _ = stop.wait_for(|value| *value).await;
    }
}

pub(super) struct RequestLifetime(pub Arc<QueryState>);

impl Drop for RequestLifetime {
    fn drop(&mut self) {
        self.0.finish("abandoned");
    }
}

/// Keep the role lock through bounded executor cancellation/rollback, including
/// when Hyper drops a handler. Never lend a connection with unread responses.
pub(super) struct SessionLease(pub Option<OwnedMutexGuard<DedicatedSession>>);

impl Drop for SessionLease {
    fn drop(&mut self) {
        if let Some(mut session) = self.0.take() {
            tokio::spawn(async move {
                if !matches!(
                    tokio::time::timeout(Duration::from_millis(800), session.wait_idle()).await,
                    Ok(Ok(()))
                ) {
                    session.close();
                }
            });
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn cancellation_is_sticky_and_terminal_status_does_not_change() {
        let state = QueryState::new();
        assert_eq!(state.cancel(), "requested");
        tokio::time::timeout(Duration::from_millis(20), state.canceled()).await.unwrap();
        state.finish("canceled");
        drop(RequestLifetime(Arc::clone(&state)));
        assert_eq!(state.status(), "canceled");
        assert_eq!(state.cancel(), "not_running");
        let abandoned = QueryState::new();
        drop(RequestLifetime(Arc::clone(&abandoned)));
        assert_eq!(abandoned.status(), "abandoned");
    }
}
