//! Drive the accepted session-store. This module does not reimplement the reducer
//! and it never sends `complete_release`.

use session_store::{
    get, transition, Endpoint, Event, IdentityTuple, Outcome, Phase, SessionState, StoreError, TransitionRequest,
};
use session_transition::{CancelTrigger, StepId, StepStatus};

#[derive(Clone, Debug)]
pub struct Cursor {
    pub revision: u64,
    pub generation: u64,
    pub now_ms: u64,
    pub phase: Phase,
    pub state: SessionState,
}

#[derive(Debug)]
pub enum DriveFail {
    Unavailable,
    UnknownCommit,
    Store(&'static str),
    Reason(String),
}

pub async fn load(
    endpoint: &Endpoint,
    user: &str,
    password: &str,
    ca_pem: &[u8],
    project_id: &str,
    session_id: &str,
) -> Result<Cursor, DriveFail> {
    let receipt = get(endpoint, user, password, ca_pem, project_id, session_id)
        .await
        .map_err(map_store)?;
    Ok(cursor_from_state(receipt.state))
}

#[allow(clippy::too_many_arguments)]
pub async fn apply_cancel(
    endpoint: &Endpoint,
    user: &str,
    password: &str,
    ca_pem: &[u8],
    project_id: &str,
    session_id: &str,
    identity: &IdentityTuple,
    cursor: &Cursor,
    event_id: &str,
    now_ms: u64,
    trigger: CancelTrigger,
) -> Result<Cursor, DriveFail> {
    apply(
        endpoint,
        user,
        password,
        ca_pem,
        project_id,
        session_id,
        identity,
        cursor,
        event_id,
        now_ms,
        &Event::Cancel { trigger },
    )
    .await
}

#[allow(clippy::too_many_arguments)]
pub async fn apply_step(
    endpoint: &Endpoint,
    user: &str,
    password: &str,
    ca_pem: &[u8],
    project_id: &str,
    session_id: &str,
    identity: &IdentityTuple,
    cursor: &Cursor,
    event_id: &str,
    now_ms: u64,
    step: StepId,
    evidence_hash: &str,
    actual_absence: bool,
) -> Result<Cursor, DriveFail> {
    if actual_absence && step != StepId::S8 {
        return Err(DriveFail::Reason("absence_only_s8".to_owned()));
    }
    apply(
        endpoint,
        user,
        password,
        ca_pem,
        project_id,
        session_id,
        identity,
        cursor,
        event_id,
        now_ms,
        &Event::RecordReleaseStep {
            step,
            status: StepStatus::Confirmed,
            evidence_hash: evidence_hash.to_owned(),
            actual_absence,
        },
    )
    .await
}

#[allow(clippy::too_many_arguments)]
async fn apply(
    endpoint: &Endpoint,
    user: &str,
    password: &str,
    ca_pem: &[u8],
    project_id: &str,
    session_id: &str,
    identity: &IdentityTuple,
    cursor: &Cursor,
    event_id: &str,
    now_ms: u64,
    event: &Event,
) -> Result<Cursor, DriveFail> {
    let request = TransitionRequest {
        endpoint,
        user,
        password,
        ca_pem,
        project_id,
        session_id,
        identity,
        expected_revision: cursor.revision,
        expected_owner_generation: cursor.generation,
        event_id,
        now_ms,
        event,
    };
    let receipt = transition(&request).await.map_err(map_store)?;
    match receipt.outcome {
        Some(Outcome::Applied | Outcome::Idempotent) => {
            let output = receipt.reducer.ok_or(DriveFail::Store("reducer_missing"))?;
            let closed_by_us = output.phase == Phase::Closed
                && matches!(event, Event::RecordReleaseStep { .. } | Event::Cancel { .. });
            if output.physical_release_certified || closed_by_us {
                return Err(DriveFail::Store("unexpected_close"));
            }
            Ok(cursor_from_state(output.state))
        }
        Some(Outcome::Refused) => Err(DriveFail::Reason(reason_label(receipt.reason))),
        None => Err(DriveFail::Store("disposition")),
    }
}

fn cursor_from_state(state: SessionState) -> Cursor {
    Cursor {
        revision: state.revision,
        generation: state.owner_generation,
        now_ms: state.last_now_ms,
        phase: state.phase,
        state,
    }
}

fn map_store(err: StoreError) -> DriveFail {
    match err {
        StoreError::Connect | StoreError::Tls => DriveFail::Unavailable,
        StoreError::CommitUnknown => DriveFail::UnknownCommit,
        other => DriveFail::Store(other.reason()),
    }
}

fn reason_label(reason: Option<session_store::Reason>) -> String {
    match reason {
        Some(reason) => serde_json::to_value(reason)
            .ok()
            .and_then(|value| value.as_str().map(str::to_owned))
            .unwrap_or_else(|| "refused".to_owned()),
        None => "refused".to_owned(),
    }
}
