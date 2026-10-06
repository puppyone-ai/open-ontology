use serde::Serialize;

use crate::model::{
    identity_ok, is_hex64, is_token, CancelTrigger, Event, FailureCode, IdentityTuple, JournalEntry,
    LastTurnOutcome, Outcome, Phase, Reason, ReleaseSteps, SessionState, StepId, StepRecord,
    StepStatus, TransitionInput, TransitionOutput, TurnState, STEPS, ABSOLUTE_BOUNDARY_MS, CONTRACT,
    DEDUP_JOURNAL_CAP, IDLE_BOUNDARY_MS, LEASE_MAX_MS, TOOL_BOUNDARY, TURN_BOUNDARY_MS,
};
use crate::sha256::{hex, sha256};

enum ApplyError {
    Refused(Reason),
    GenerationOverflow,
}

#[derive(Serialize)]
struct Fingerprint<'a> {
    event_id: &'a str,
    identity: &'a IdentityTuple,
    event: &'a Event,
}

#[must_use]
pub fn transition(input: &TransitionInput) -> TransitionOutput {
    if let Err(reason) = validate_state(&input.state).and_then(|()| validate_command(input)) {
        return finish(Outcome::Refused, Some(reason), input.state.clone(), &input.identity);
    }
    if input.now_ms < input.state.last_now_ms {
        return finish(
            Outcome::Refused,
            Some(Reason::TimeBackward),
            input.state.clone(),
            &input.identity,
        );
    }
    let Ok(fingerprint) = fingerprint_hex(input) else {
        return finish(
            Outcome::Refused,
            Some(Reason::InputInvalid),
            input.state.clone(),
            &input.identity,
        );
    };
    if let Some(existing) = input
        .state
        .journal
        .iter()
        .find(|entry| entry.event_id == input.event_id)
    {
        let reason = if existing.canonical_sha256 == fingerprint {
            None
        } else {
            Some(Reason::EventIdConflict)
        };
        let outcome = if reason.is_none() {
            Outcome::Idempotent
        } else {
            Outcome::Refused
        };
        return finish(outcome, reason, input.state.clone(), &input.identity);
    }
    if let Some(bound) = &input.state.bound
        && bound != &input.identity
    {
        return finish(
            Outcome::Refused,
            Some(Reason::NewSessionRequired),
            input.state.clone(),
            &input.identity,
        );
    }
    if input.expected_revision != input.state.revision
        || input.expected_owner_generation != input.state.owner_generation
    {
        return finish(
            Outcome::Refused,
            Some(Reason::Stale),
            input.state.clone(),
            &input.identity,
        );
    }
    let mut next = match apply(input) {
        Ok(state) => state,
        Err(ApplyError::GenerationOverflow) => {
            let reason = if input.state.journal.len() == DEDUP_JOURNAL_CAP {
                Reason::DedupExhausted
            } else {
                Reason::GenerationOverflow
            };
            return finish(Outcome::Refused, Some(reason), input.state.clone(), &input.identity);
        }
        Err(ApplyError::Refused(reason)) => {
            return finish(Outcome::Refused, Some(reason), input.state.clone(), &input.identity);
        }
    };
    if input.state.journal.len() == DEDUP_JOURNAL_CAP {
        return finish(
            Outcome::Refused,
            Some(Reason::DedupExhausted),
            input.state.clone(),
            &input.identity,
        );
    }
    let Some(revision) = input.state.revision.checked_add(1) else {
        return finish(
            Outcome::Refused,
            Some(Reason::RevisionOverflow),
            input.state.clone(),
            &input.identity,
        );
    };
    next.revision = revision;
    next.last_now_ms = input.now_ms;
    next.journal.push(JournalEntry {
        event_id: input.event_id.clone(),
        canonical_sha256: fingerprint,
    });
    finish(Outcome::Applied, None, next, &input.identity)
}

#[allow(clippy::large_types_passed_by_value)]
fn finish(
    outcome: Outcome,
    reason: Option<Reason>,
    state: SessionState,
    identity: &IdentityTuple,
) -> TransitionOutput {
    let same_workspace_allowed = matches!(outcome, Outcome::Applied | Outcome::Idempotent)
        && state.workspace_id.is_some()
        && state.bound.as_ref() == Some(identity)
        && matches!(state.phase, Phase::Ready | Phase::Running | Phase::Idle);
    TransitionOutput {
        contract: CONTRACT.to_owned(),
        outcome,
        reason,
        revision: state.revision,
        owner_generation: state.owner_generation,
        phase: state.phase,
        new_session_required: reason == Some(Reason::NewSessionRequired),
        same_workspace_allowed,
        physical_release_certified: false,
        database_used: false,
        auth_provider_used: false,
        runtime_used: false,
        state,
    }
}

fn fingerprint_hex(input: &TransitionInput) -> Result<String, ()> {
    let bytes = serde_json::to_vec(&Fingerprint {
        event_id: &input.event_id,
        identity: &input.identity,
        event: &input.event,
    })
    .map_err(|_| ())?;
    sha256(&bytes).map(|digest| hex(&digest)).ok_or(())
}

fn validate_command(input: &TransitionInput) -> Result<(), Reason> {
    if input.contract != CONTRACT || !is_token(&input.event_id) || !identity_ok(&input.identity) {
        return Err(Reason::InputInvalid);
    }
    if !event_syntax_ok(&input.event) {
        return Err(Reason::InputInvalid);
    }
    Ok(())
}

fn event_syntax_ok(event: &Event) -> bool {
    match event {
        Event::BeginBind { owner_id, .. } | Event::Heartbeat { owner_id, .. } | Event::Takeover { owner_id, .. } => {
            is_token(owner_id)
        }
        Event::CompleteBind { workspace_id } => is_token(workspace_id),
        Event::BeginTurn { turn_id } | Event::FinishTurn { turn_id } | Event::TurnTimeout { turn_id } => {
            is_token(turn_id)
        }
        Event::ToolStart { turn_id, tool_id } | Event::ToolFinish { turn_id, tool_id } => {
            is_token(turn_id) && is_token(tool_id)
        }
        Event::Cancel { .. } | Event::IdleTimeout | Event::AbsoluteTimeout => true,
        Event::RecordReleaseStep { evidence_hash, .. } => is_hex64(evidence_hash),
        Event::CompleteRelease { steps } => STEPS.iter().all(|step| match steps.get(*step) {
            Some(record) => is_hex64(&record.evidence_hash),
            None => true,
        }),
    }
}

fn validate_state(state: &SessionState) -> Result<(), Reason> {
    if !is_token(&state.session_id) || !is_token(&state.conversation_id) {
        return Err(Reason::StateInvalid);
    }
    if state.journal.len() > DEDUP_JOURNAL_CAP || !journal_ok(&state.journal) || !tools_ok(state) {
        return Err(Reason::StateInvalid);
    }
    if !release_records_ok(&state.release) {
        return Err(Reason::StateInvalid);
    }
    let bound_ok = match &state.bound {
        Some(identity) => identity_ok(identity),
        None => true,
    };
    if !bound_ok {
        return Err(Reason::StateInvalid);
    }
    let generation_ok = if state.owner_generation == 0 {
        state.owner_id.is_empty() && state.bound.is_none() && state.workspace_id.is_none()
    } else {
        is_token(&state.owner_id) && state.bound.is_some()
    };
    if !generation_ok {
        return Err(Reason::StateInvalid);
    }
    if let Some(workspace) = &state.workspace_id
        && !is_token(workspace)
    {
        return Err(Reason::StateInvalid);
    }
    if phase_shape_ok(state) {
        Ok(())
    } else {
        Err(Reason::StateInvalid)
    }
}

fn journal_ok(journal: &[JournalEntry]) -> bool {
    let mut seen = Vec::<&str>::new();
    for entry in journal {
        if !is_token(&entry.event_id) || !is_hex64(&entry.canonical_sha256) {
            return false;
        }
        if seen.iter().any(|id| *id == entry.event_id) {
            return false;
        }
        seen.push(&entry.event_id);
    }
    true
}

fn tools_ok(state: &SessionState) -> bool {
    if state.tool_starts > TOOL_BOUNDARY {
        return false;
    }
    let Ok(count) = usize::try_from(state.tool_starts) else {
        return false;
    };
    if count != state.started_tool_ids.len() {
        return false;
    }
    if !strictly_sorted_tokens(&state.started_tool_ids) || !strictly_sorted_tokens(&state.open_tool_ids) {
        return false;
    }
    state
        .open_tool_ids
        .iter()
        .all(|id| state.started_tool_ids.binary_search(id).is_ok())
}

fn strictly_sorted_tokens(ids: &[String]) -> bool {
    ids.iter().all(|id| is_token(id)) && ids.windows(2).all(|pair| pair[0] < pair[1])
}

fn release_records_ok(release: &crate::model::ReleaseRecord) -> bool {
    STEPS.iter().all(|step| match release.get(*step) {
        None => true,
        Some(record) => check_step(*step, record).is_ok(),
    })
}

fn phase_shape_ok(state: &SessionState) -> bool {
    let tools_clear = state.turn.is_none()
        && state.tool_starts == 0
        && state.started_tool_ids.is_empty()
        && state.open_tool_ids.is_empty();
    match state.phase {
        Phase::Requested => {
            state.owner_generation == 0
                && state.lease_expires_at_ms == 0
                && tools_clear
                && state.last_turn_outcome.is_none()
                && state.failure.is_none()
                && state.release.all_empty()
        }
        Phase::Binding => {
            state.owner_generation >= 1
                && state.workspace_id.is_none()
                && tools_clear
                && state.last_turn_outcome.is_none()
                && state.failure.is_none()
                && state.release.all_empty()
        }
        Phase::Ready => {
            state.owner_generation >= 1
                && state.workspace_id.is_some()
                && tools_clear
                && state.last_turn_outcome.is_none()
                && state.failure.is_none()
                && state.release.all_empty()
        }
        Phase::Running => {
            state.owner_generation >= 1
                && state.workspace_id.is_some()
                && state.failure.is_none()
                && state.last_turn_outcome.is_none()
                && state.release.all_empty()
                && match &state.turn {
                    Some(turn) => is_token(&turn.turn_id),
                    None => false,
                }
        }
        Phase::Idle => {
            state.owner_generation >= 1
                && state.workspace_id.is_some()
                && tools_clear
                && state.last_turn_outcome.is_some()
                && state.failure.is_none()
                && state.release.all_empty()
        }
        Phase::Releasing => state.failure.is_some() && tools_clear,
        Phase::Closed => {
            state.failure.is_some() && tools_clear && release_complete_for_close(&state.release)
        }
    }
}

fn release_complete_for_close(release: &crate::model::ReleaseRecord) -> bool {
    let Some(s8) = release.get(StepId::S8) else {
        return false;
    };
    if s8.status != StepStatus::Confirmed || !s8.actual_absence {
        return false;
    }
    STEPS.iter().all(|step| release.get(*step).is_some())
}

fn apply(input: &TransitionInput) -> Result<SessionState, ApplyError> {
    match &input.event {
        Event::BeginBind { owner_id, lease_expires_at_ms } => {
            begin_bind(input, owner_id, *lease_expires_at_ms)
        }
        Event::CompleteBind { workspace_id } => {
            complete_bind(input, workspace_id).map_err(ApplyError::Refused)
        }
        Event::BeginTurn { turn_id } => begin_turn(input, turn_id).map_err(ApplyError::Refused),
        Event::FinishTurn { turn_id } => finish_turn(input, turn_id).map_err(ApplyError::Refused),
        Event::ToolStart { turn_id, tool_id } => {
            tool_start(input, turn_id, tool_id).map_err(ApplyError::Refused)
        }
        Event::ToolFinish { turn_id, tool_id } => {
            tool_finish(input, turn_id, tool_id).map_err(ApplyError::Refused)
        }
        Event::Heartbeat { owner_id, lease_expires_at_ms } => {
            heartbeat(input, owner_id, *lease_expires_at_ms).map_err(ApplyError::Refused)
        }
        Event::Takeover { owner_id, lease_expires_at_ms } => {
            takeover(input, owner_id, *lease_expires_at_ms)
        }
        Event::Cancel { trigger } => cancel(input, *trigger).map_err(ApplyError::Refused),
        Event::TurnTimeout { turn_id } => turn_timeout(input, turn_id).map_err(ApplyError::Refused),
        Event::IdleTimeout => idle_timeout(input).map_err(ApplyError::Refused),
        Event::AbsoluteTimeout => absolute_timeout(input).map_err(ApplyError::Refused),
        Event::RecordReleaseStep {
            step,
            status,
            evidence_hash,
            actual_absence,
        } => record_step(input, *step, *status, evidence_hash, *actual_absence)
            .map_err(ApplyError::Refused),
        Event::CompleteRelease { steps } => {
            complete_release(input, steps).map_err(ApplyError::Refused)
        }
    }
}

fn not_admitted(phase: Phase) -> Reason {
    match phase {
        Phase::Releasing => Reason::AdmissionStopped,
        Phase::Closed => Reason::Closed,
        _ => Reason::WrongPhase,
    }
}

fn lease_window(now_ms: u64, expiry: u64) -> Result<(), Reason> {
    if expiry <= now_ms {
        return Err(Reason::LeaseNotFuture);
    }
    match expiry.checked_sub(now_ms) {
        Some(delta) if delta <= LEASE_MAX_MS => Ok(()),
        Some(_) => Err(Reason::LeaseTooLong),
        None => Err(Reason::LeaseNotFuture),
    }
}

fn lease_live(state: &SessionState, now_ms: u64) -> bool {
    now_ms < state.lease_expires_at_ms
}

fn at_boundary(now_ms: u64, started_ms: u64, bound: u64) -> bool {
    match now_ms.checked_sub(started_ms) {
        Some(elapsed) => elapsed >= bound,
        None => false,
    }
}

fn require_live_lease(state: &SessionState, now_ms: u64) -> Result<(), Reason> {
    if lease_live(state, now_ms) {
        Ok(())
    } else {
        Err(Reason::LeaseExpired)
    }
}

fn clear_turn(state: &mut SessionState) {
    state.turn = None;
    state.tool_starts = 0;
    state.started_tool_ids.clear();
    state.open_tool_ids.clear();
}

fn enter_releasing(state: &mut SessionState, failure: FailureCode) {
    clear_turn(state);
    state.phase = Phase::Releasing;
    state.failure = Some(failure);
}

fn begin_bind(
    input: &TransitionInput,
    owner_id: &str,
    lease_expires_at_ms: u64,
) -> Result<SessionState, ApplyError> {
    if input.state.phase != Phase::Requested {
        return Err(ApplyError::Refused(not_admitted(input.state.phase)));
    }
    lease_window(input.now_ms, lease_expires_at_ms).map_err(ApplyError::Refused)?;
    let Some(generation) = input.state.owner_generation.checked_add(1) else {
        return Err(ApplyError::GenerationOverflow);
    };
    let mut next = input.state.clone();
    next.phase = Phase::Binding;
    next.owner_id = owner_id.to_owned();
    next.owner_generation = generation;
    next.lease_expires_at_ms = lease_expires_at_ms;
    next.bound = Some(input.identity.clone());
    next.session_started_at_ms = input.now_ms;
    next.last_activity_at_ms = input.now_ms;
    Ok(next)
}

fn complete_bind(input: &TransitionInput, workspace_id: &str) -> Result<SessionState, Reason> {
    if input.state.phase != Phase::Binding {
        return Err(not_admitted(input.state.phase));
    }
    require_live_lease(&input.state, input.now_ms)?;
    if at_boundary(input.now_ms, input.state.session_started_at_ms, ABSOLUTE_BOUNDARY_MS) {
        return Err(Reason::AbsoluteBoundary);
    }
    let mut next = input.state.clone();
    next.phase = Phase::Ready;
    next.workspace_id = Some(workspace_id.to_owned());
    next.last_activity_at_ms = input.now_ms;
    Ok(next)
}

fn begin_turn(input: &TransitionInput, turn_id: &str) -> Result<SessionState, Reason> {
    if !matches!(input.state.phase, Phase::Ready | Phase::Idle) {
        return Err(not_admitted(input.state.phase));
    }
    require_live_lease(&input.state, input.now_ms)?;
    if at_boundary(input.now_ms, input.state.session_started_at_ms, ABSOLUTE_BOUNDARY_MS) {
        return Err(Reason::AbsoluteBoundary);
    }
    if at_boundary(input.now_ms, input.state.last_activity_at_ms, IDLE_BOUNDARY_MS) {
        return Err(Reason::IdleBoundary);
    }
    let mut next = input.state.clone();
    next.phase = Phase::Running;
    next.turn = Some(TurnState {
        turn_id: turn_id.to_owned(),
        started_at_ms: input.now_ms,
    });
    next.tool_starts = 0;
    next.started_tool_ids.clear();
    next.open_tool_ids.clear();
    next.last_turn_outcome = None;
    next.last_activity_at_ms = input.now_ms;
    Ok(next)
}

fn running_turn<'a>(input: &'a TransitionInput, turn_id: &str) -> Result<&'a TurnState, Reason> {
    if input.state.phase != Phase::Running {
        return Err(not_admitted(input.state.phase));
    }
    require_live_lease(&input.state, input.now_ms)?;
    let Some(turn) = &input.state.turn else {
        return Err(Reason::StateInvalid);
    };
    if turn.turn_id != turn_id {
        return Err(Reason::TurnMismatch);
    }
    if at_boundary(input.now_ms, input.state.session_started_at_ms, ABSOLUTE_BOUNDARY_MS) {
        return Err(Reason::AbsoluteBoundary);
    }
    if at_boundary(input.now_ms, turn.started_at_ms, TURN_BOUNDARY_MS) {
        return Err(Reason::TurnBoundary);
    }
    Ok(turn)
}

fn finish_turn(input: &TransitionInput, turn_id: &str) -> Result<SessionState, Reason> {
    let _turn = running_turn(input, turn_id)?;
    if !input.state.open_tool_ids.is_empty() {
        return Err(Reason::ToolsOpen);
    }
    let mut next = input.state.clone();
    clear_turn(&mut next);
    next.phase = Phase::Idle;
    next.last_turn_outcome = Some(LastTurnOutcome::Finished);
    next.last_activity_at_ms = input.now_ms;
    Ok(next)
}

fn tool_start(input: &TransitionInput, turn_id: &str, tool_id: &str) -> Result<SessionState, Reason> {
    let _turn = running_turn(input, turn_id)?;
    if input.state.tool_starts >= TOOL_BOUNDARY {
        return Err(Reason::ToolBoundary);
    }
    if input.state.started_tool_ids.binary_search_by(|probe| probe.as_str().cmp(tool_id)).is_ok() {
        return Err(Reason::ToolIdReused);
    }
    let mut next = input.state.clone();
    let Some(starts) = next.tool_starts.checked_add(1) else {
        return Err(Reason::ToolBoundary);
    };
    next.tool_starts = starts;
    insert_sorted(&mut next.started_tool_ids, tool_id);
    insert_sorted(&mut next.open_tool_ids, tool_id);
    next.last_activity_at_ms = input.now_ms;
    Ok(next)
}

fn tool_finish(input: &TransitionInput, turn_id: &str, tool_id: &str) -> Result<SessionState, Reason> {
    let _turn = running_turn(input, turn_id)?;
    if input.state.open_tool_ids.binary_search_by(|probe| probe.as_str().cmp(tool_id)).is_err() {
        return Err(Reason::ToolNotOpen);
    }
    let mut next = input.state.clone();
    remove_sorted(&mut next.open_tool_ids, tool_id);
    next.last_activity_at_ms = input.now_ms;
    Ok(next)
}

fn heartbeat(input: &TransitionInput, owner_id: &str, lease_expires_at_ms: u64) -> Result<SessionState, Reason> {
    if !matches!(
        input.state.phase,
        Phase::Binding | Phase::Ready | Phase::Running | Phase::Idle | Phase::Releasing
    ) {
        return Err(match input.state.phase {
            Phase::Closed => Reason::Closed,
            _ => Reason::WrongPhase,
        });
    }
    if input.state.owner_id != owner_id {
        return Err(Reason::OwnerMismatch);
    }
    if !lease_live(&input.state, input.now_ms) {
        return Err(Reason::LeaseExpired);
    }
    lease_window(input.now_ms, lease_expires_at_ms)?;
    let mut next = input.state.clone();
    next.lease_expires_at_ms = lease_expires_at_ms;
    Ok(next)
}

fn takeover(
    input: &TransitionInput,
    owner_id: &str,
    lease_expires_at_ms: u64,
) -> Result<SessionState, ApplyError> {
    if !matches!(
        input.state.phase,
        Phase::Binding | Phase::Ready | Phase::Running | Phase::Idle | Phase::Releasing
    ) {
        return Err(ApplyError::Refused(match input.state.phase {
            Phase::Closed => Reason::Closed,
            _ => Reason::WrongPhase,
        }));
    }
    if input.state.owner_generation == 0 {
        return Err(ApplyError::Refused(Reason::WrongPhase));
    }
    if input.now_ms < input.state.lease_expires_at_ms {
        return Err(ApplyError::Refused(Reason::LeaseHeld));
    }
    lease_window(input.now_ms, lease_expires_at_ms).map_err(ApplyError::Refused)?;
    let Some(generation) = input.state.owner_generation.checked_add(1) else {
        return Err(ApplyError::GenerationOverflow);
    };
    let mut next = input.state.clone();
    if next.phase != Phase::Releasing {
        enter_releasing(&mut next, FailureCode::OwnerLost);
    }
    next.owner_id = owner_id.to_owned();
    next.owner_generation = generation;
    next.lease_expires_at_ms = lease_expires_at_ms;
    Ok(next)
}

fn cancel(input: &TransitionInput, trigger: CancelTrigger) -> Result<SessionState, Reason> {
    let failure = match trigger {
        CancelTrigger::User => FailureCode::UserCancel,
        CancelTrigger::Admin => FailureCode::AdminTerminate,
        CancelTrigger::EntryLost => FailureCode::EntryLost,
    };
    match input.state.phase {
        Phase::Releasing => Err(Reason::AlreadyReleasing),
        Phase::Closed => Err(Reason::Closed),
        Phase::Requested | Phase::Binding | Phase::Ready | Phase::Running | Phase::Idle => {
            let mut next = input.state.clone();
            enter_releasing(&mut next, failure);
            Ok(next)
        }
    }
}

fn turn_timeout(input: &TransitionInput, turn_id: &str) -> Result<SessionState, Reason> {
    match input.state.phase {
        Phase::Releasing => return Err(Reason::AlreadyReleasing),
        Phase::Closed => return Err(Reason::Closed),
        Phase::Running => {}
        _ => return Err(Reason::WrongPhase),
    }
    let Some(turn) = &input.state.turn else {
        return Err(Reason::StateInvalid);
    };
    if turn.turn_id != turn_id {
        return Err(Reason::TurnMismatch);
    }
    if !at_boundary(input.now_ms, turn.started_at_ms, TURN_BOUNDARY_MS) {
        return Err(Reason::TurnNotReached);
    }
    let mut next = input.state.clone();
    clear_turn(&mut next);
    next.phase = Phase::Idle;
    next.last_turn_outcome = Some(LastTurnOutcome::TimedOut);
    next.last_activity_at_ms = input.now_ms;
    Ok(next)
}

fn idle_timeout(input: &TransitionInput) -> Result<SessionState, Reason> {
    match input.state.phase {
        Phase::Ready | Phase::Idle => {}
        Phase::Releasing => return Err(Reason::AlreadyReleasing),
        Phase::Closed => return Err(Reason::Closed),
        _ => return Err(Reason::WrongPhase),
    }
    if !at_boundary(input.now_ms, input.state.last_activity_at_ms, IDLE_BOUNDARY_MS) {
        return Err(Reason::IdleNotReached);
    }
    let mut next = input.state.clone();
    enter_releasing(&mut next, FailureCode::IdleTimeout);
    Ok(next)
}

fn absolute_timeout(input: &TransitionInput) -> Result<SessionState, Reason> {
    match input.state.phase {
        Phase::Requested => return Err(Reason::NotStarted),
        Phase::Releasing => return Err(Reason::AlreadyReleasing),
        Phase::Closed => return Err(Reason::Closed),
        Phase::Binding | Phase::Ready | Phase::Running | Phase::Idle => {}
    }
    if !at_boundary(input.now_ms, input.state.session_started_at_ms, ABSOLUTE_BOUNDARY_MS) {
        return Err(Reason::AbsoluteNotReached);
    }
    let mut next = input.state.clone();
    enter_releasing(&mut next, FailureCode::AbsoluteTimeout);
    Ok(next)
}

fn check_step(step: StepId, record: &StepRecord) -> Result<(), Reason> {
    if !is_hex64(&record.evidence_hash) {
        return Err(Reason::InputInvalid);
    }
    if record.status == StepStatus::Skipped {
        return if step != StepId::S5 {
            Err(Reason::StepStatus)
        } else if record.actual_absence {
            Err(Reason::AbsenceOnlyS8)
        } else {
            Ok(())
        };
    }
    if record.actual_absence && step != StepId::S8 {
        return Err(Reason::AbsenceOnlyS8);
    }
    Ok(())
}

fn record_step(
    input: &TransitionInput,
    step: StepId,
    status: StepStatus,
    evidence_hash: &str,
    actual_absence: bool,
) -> Result<SessionState, Reason> {
    if input.state.phase == Phase::Closed {
        return Err(Reason::Closed);
    }
    if input.state.phase != Phase::Releasing {
        return Err(Reason::WrongPhase);
    }
    let record = StepRecord {
        status,
        evidence_hash: evidence_hash.to_owned(),
        actual_absence,
    };
    check_step(step, &record)?;
    let mut next = input.state.clone();
    next.release.set(step, record);
    Ok(next)
}

fn complete_release(input: &TransitionInput, steps: &ReleaseSteps) -> Result<SessionState, Reason> {
    if input.state.phase == Phase::Closed {
        return Err(Reason::Closed);
    }
    if input.state.phase != Phase::Releasing {
        return Err(Reason::WrongPhase);
    }
    for step in STEPS {
        let Some(record) = steps.get(step) else {
            return Err(Reason::ReleaseIncomplete);
        };
        check_step(step, record)?;
        if let Some(existing) = input.state.release.get(step)
            && existing != record
        {
            return Err(Reason::ReleaseMismatch);
        }
    }
    let Some(s8) = steps.get(StepId::S8) else {
        return Err(Reason::ReleaseIncomplete);
    };
    if s8.status != StepStatus::Confirmed || !s8.actual_absence {
        return Err(Reason::S8NotAbsent);
    }
    let mut next = input.state.clone();
    for step in STEPS {
        let Some(record) = steps.get(step) else {
            return Err(Reason::ReleaseIncomplete);
        };
        next.release.set(step, record.clone());
    }
    next.phase = Phase::Closed;
    Ok(next)
}

fn insert_sorted(ids: &mut Vec<String>, id: &str) {
    match ids.binary_search_by(|probe| probe.as_str().cmp(id)) {
        Ok(_) => {}
        Err(pos) => ids.insert(pos, id.to_owned()),
    }
}

fn remove_sorted(ids: &mut Vec<String>, id: &str) {
    if let Ok(pos) = ids.binary_search_by(|probe| probe.as_str().cmp(id)) {
        ids.remove(pos);
    }
}
