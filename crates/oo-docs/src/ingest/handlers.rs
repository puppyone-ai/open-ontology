//! Binds `sync` and `hash` jobs to the scheduler.

use std::collections::HashMap;
use std::sync::Arc;

use sqlx::PgPool;

use super::reconcile::{SyncStatus, run_sync};
use super::{IngestError, IngestLimits, SourceFactory, SyncFlavor, hashjob};
use crate::jobs::scheduler::{HandlerFuture, JobCtx, JobHandler, Outcome};
use crate::jobs::{FailureClass, JobKind};

/// `sync` job handler.
pub struct SyncHandler<F: SourceFactory> {
    pool: PgPool,
    factory: Arc<F>,
    limits: Arc<IngestLimits>,
}

impl<F: SourceFactory> JobHandler for SyncHandler<F> {
    fn run(&self, ctx: JobCtx) -> HandlerFuture<'_> {
        Box::pin(async move {
            let Some((drive, _hint, flavor)) = SyncFlavor::from_key(&ctx.job.dedupe_key) else {
                return Outcome::failed(FailureClass::Permanent, "DOCS_ENGINE_INTERNAL");
            };
            match run_sync(
                &self.pool,
                &*self.factory,
                &drive,
                flavor,
                &self.limits,
                ctx.cancel_flag(),
            )
            .await
            {
                Ok(report) => match report.status {
                    SyncStatus::Complete | SyncStatus::Held => Outcome::Done,
                    SyncStatus::Failed if ctx.cancel_requested() => Outcome::Cancelled,
                    SyncStatus::Failed => {
                        let code = report.error_code.unwrap_or_default();
                        let permanent = matches!(
                            code.as_str(),
                            "DOCS_SOURCE_ROOT_MISSING"
                                | "DOCS_SOURCE_INVALID_CONFIG"
                                | "DOCS_SOURCE_AUTH_FAILED"
                                | "DOCS_SOURCE_PERMISSION_DENIED"
                        );
                        let class = if permanent {
                            FailureClass::Permanent
                        } else {
                            FailureClass::Transient
                        };
                        Outcome::failed(class, &code)
                    }
                },
                // Someone else is syncing, or there is nothing to sync.
                Err(
                    IngestError::SyncRunning
                    | IngestError::DriveNotFound
                    | IngestError::DriveNotRunnable(_)
                    | IngestError::ConfirmMismatch { .. },
                ) => Outcome::Done,
                Err(_) => Outcome::failed(FailureClass::Transient, "DOCS_ENGINE_DB_UNAVAILABLE"),
            }
        })
    }
}

/// Handlers for the ingest stage: `sync` and `hash`.
pub fn handlers<F: SourceFactory>(
    pool: &PgPool,
    factory: Arc<F>,
    limits: Arc<IngestLimits>,
) -> HashMap<JobKind, Arc<dyn JobHandler>> {
    let mut map: HashMap<JobKind, Arc<dyn JobHandler>> = HashMap::new();
    map.insert(
        JobKind::Sync,
        Arc::new(SyncHandler {
            pool: pool.clone(),
            factory: factory.clone(),
            limits: limits.clone(),
        }),
    );
    map.insert(
        JobKind::Hash,
        Arc::new(hashjob::HashHandler {
            pool: pool.clone(),
            factory,
            limits,
        }),
    );
    map
}
