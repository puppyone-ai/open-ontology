//! Retention admission from the oldest readable event, never server settings.
use std::{future::poll_fn, pin::Pin, time::Duration};

use futures_util::Stream;
use mysql_async::{BinlogStreamRequest, prelude::Queryable};
use oo_connectors::{
    engine::{Requirement, RequirementKind, SourceEngine, Watermark},
    mysql::{MysqlConfig, MysqlEngine},
};

use crate::PreviewError;

#[derive(Debug)]
pub struct Retention {
    pub earliest: Watermark,
    pub oldest_event_unix: Option<u32>,
    pub server_unix: u64,
    pub required_seconds: u64,
    /// A short or unmeasured history requires an explicit new baseline;
    /// this source-only result does not allocate or publish that generation.
    pub rebaseline: Option<Requirement>,
}

impl Retention {
    pub fn require_sufficient(self) -> Result<Self, Requirement> {
        match &self.rebaseline {
            Some(requirement) => Err(requirement.clone()),
            None => Ok(self),
        }
    }
}

fn requirement(observed: Option<u64>, required: u64) -> Requirement {
    Requirement {
        kind: RequirementKind::Retention,
        name: "actual_binlog_history_new_generation_rebaseline".into(),
        observed: observed.map(|v| v.to_string()),
        required: format!("{required} seconds (longest cycle + recovery)"),
        dba_statement: None,
    }
}

/// Uses a separate read-only replication connection. No ACK, FLUSH, source
/// mutation, target access or configured-retention fallback is performed.
pub async fn inspect(
    engine: &mut MysqlEngine,
    config: &MysqlConfig,
    password: &str,
    client_server_id: u32,
    longest_cycle_seconds: u64,
    recovery_seconds: u64,
) -> Result<Retention, PreviewError> {
    let required = longest_cycle_seconds
        .checked_add(recovery_seconds)
        .ok_or_else(|| PreviewError::new("config", "retention_interval"))?;
    let error = || PreviewError::new("c1", "retention_measurement");
    // Includes the replica commit-order guard, refreshed at the boundary.
    if engine.watermark().await.map_err(|_| error())?.is_none() {
        return Err(error());
    }
    let earliest = engine.earliest_binlog().await.map_err(|_| error())?;
    let Watermark::MysqlFilePos { file, position } = &earliest else {
        return Err(error());
    };
    let mut conn = oo_connectors::mysql::connect(config, password).await?;
    let (server_unix, server_id): (u64, u32) = conn
        .query_first("SELECT UNIX_TIMESTAMP(), @@GLOBAL.server_id")
        .await
        .map_err(|_| error())?
        .ok_or_else(error)?;
    if client_server_id == 0 || client_server_id == server_id {
        return Err(PreviewError::new("config", "server_id"));
    }
    let mut stream = Box::pin(
        conn.get_binlog_stream(
            BinlogStreamRequest::new(client_server_id)
                .with_filename(file.as_bytes())
                .with_pos(*position)
                .with_non_blocking(),
        )
        .await
        .map_err(|_| error())?,
    );
    let measured = tokio::time::timeout(Duration::from_secs(10), async {
        while let Some(event) = poll_fn(|cx| stream.as_mut().poll_next(cx)).await {
            let event = event.map_err(|_| error())?;
            let timestamp = event.header().timestamp();
            // The synthetic rotation announcing the starting file is undated.
            if timestamp > 0 {
                return Ok::<_, PreviewError>(Some(timestamp));
            }
        }
        Ok(None)
    })
    .await;
    Pin::into_inner(stream).close().await.map_err(|_| error())?;
    let oldest_event_unix = measured.map_err(|_| error())??;
    // A purge racing subscription cannot turn an obsolete measurement into
    // admission. Retry from the new oldest file instead.
    if engine.earliest_binlog().await.map_err(|_| error())? != earliest {
        return Err(PreviewError::new("c1", "retention_inventory_changed_retry"));
    }
    let age = oldest_event_unix.and_then(|oldest| server_unix.checked_sub(u64::from(oldest)));
    Ok(Retention {
        earliest,
        oldest_event_unix,
        server_unix,
        required_seconds: required,
        rebaseline: age
            .filter(|age| *age >= required)
            .is_none()
            .then(|| requirement(age, required)),
    })
}
