//! One serial worker per project. Claim, run the pinned producer, then finish.

use std::sync::Arc;
use std::time::Duration;

use crate::config::Config;
use crate::producers;
use crate::store::{Finish, Store};

pub fn spawn(config: Arc<Config>, store: Store) {
    tokio::spawn(async move {
        loop {
            match store.claim_next().await {
                Ok(Some(operation)) => {
                    let produced = producers::execute(&config, &store, &operation).await;
                    let finish = Finish {
                        operation_id: operation.operation_id,
                        success: produced.success,
                        producer_code: produced.producer_code,
                        producer_detail: produced.producer_detail,
                        rows: produced.rows,
                        child_started: produced.child_started,
                        catalog: produced.catalog,
                        profile: produced.profile,
                    };
                    for _ in 0..5 {
                        if store.finish(&finish).await.is_ok() {
                            break;
                        }
                        tokio::time::sleep(Duration::from_millis(200)).await;
                    }
                }
                Ok(None) => tokio::time::sleep(Duration::from_millis(200)).await,
                Err(_) => tokio::time::sleep(Duration::from_millis(500)).await,
            }
        }
    });
}
