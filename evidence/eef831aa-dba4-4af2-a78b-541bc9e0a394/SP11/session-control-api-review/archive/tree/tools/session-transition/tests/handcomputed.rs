#![forbid(unsafe_code)]

use session_transition::{
    transition, CancelTrigger, Event, FailureCode, IdentityTuple, JournalEntry, LastTurnOutcome,
    Outcome, Phase, Reason, ReleaseRecord, ReleaseSteps, SessionState, StepId, StepRecord,
    StepStatus, TransitionInput, TransitionOutput, TurnState, CONTRACT, DEDUP_JOURNAL_CAP,
    IDLE_BOUNDARY_MS, ABSOLUTE_BOUNDARY_MS, TURN_BOUNDARY_MS,
};

const ABC: &str = "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad";
const EMPTY: &str = "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855";

fn alice() -> IdentityTuple {
    IdentityTuple {
        project_id: "project-1".to_owned(),
        principal_id: "alice".to_owned(),
        role: "analyst".to_owned(),
        claims_hash: ABC.to_owned(),
        policy_version: "pol-1".to_owned(),
        knowledge_version: "know-1".to_owned(),
        config_version: "cfg-1".to_owned(),
    }
}

fn requested() -> SessionState {
    SessionState {
        session_id: "sess-1".to_owned(),
        conversation_id: "conv-1".to_owned(),
        phase: Phase::Requested,
        revision: 0,
        owner_id: String::new(),
        owner_generation: 0,
        lease_expires_at_ms: 0,
        bound: None,
        workspace_id: None,
        session_started_at_ms: 0,
        last_activity_at_ms: 0,
        last_now_ms: 0,
        turn: None,
        tool_starts: 0,
        started_tool_ids: Vec::new(),
        open_tool_ids: Vec::new(),
        last_turn_outcome: None,
        failure: None,
        release: ReleaseRecord::default(),
        journal: Vec::new(),
    }
}

fn ready_at(revision: u64, generation: u64, last_activity: u64, started: u64, lease: u64, last_now: u64) -> SessionState {
    SessionState {
        phase: Phase::Ready,
        revision,
        owner_id: "entry-1".to_owned(),
        owner_generation: generation,
        lease_expires_at_ms: lease,
        bound: Some(alice()),
        workspace_id: Some("ws-1".to_owned()),
        session_started_at_ms: started,
        last_activity_at_ms: last_activity,
        last_now_ms: last_now,
        ..requested()
    }
}

fn running_with(count: u32, turn_started: u64, lease: u64) -> SessionState {
    let mut state = ready_at(10, 1, 0, 0, lease, 0);
    state.phase = Phase::Running;
    state.turn = Some(TurnState { turn_id: "t1".to_owned(), started_at_ms: turn_started });
    state.tool_starts = count;
    state.started_tool_ids = (0..count).map(|index| format!("tool-{index:02}")).collect();
    state
}

fn releasing(revision: u64) -> SessionState {
    let mut state = ready_at(revision, 1, 0, 0, 50_000, 0);
    state.phase = Phase::Releasing;
    state.failure = Some(FailureCode::UserCancel);
    state
}

fn call(
    state: &SessionState,
    identity: &IdentityTuple,
    expected_revision: u64,
    expected_generation: u64,
    event_id: &str,
    now_ms: u64,
    event: Event,
) -> TransitionOutput {
    transition(&TransitionInput {
        contract: CONTRACT.to_owned(),
        state: state.clone(),
        identity: identity.clone(),
        expected_revision,
        expected_owner_generation: expected_generation,
        event_id: event_id.to_owned(),
        now_ms,
        event,
    })
}

fn assert_same(out: &TransitionOutput, before: &SessionState, revision: u64, reason: Reason) {
    assert_eq!(out.outcome, Outcome::Refused);
    assert_eq!(out.reason, Some(reason));
    assert_eq!(out.revision, revision);
    assert_eq!(out.state.revision, revision);
    assert_eq!(out.state, *before);
    assert!(!out.physical_release_certified);
    assert!(!out.database_used);
    assert!(!out.auth_provider_used);
    assert!(!out.runtime_used);
    assert!(!out.same_workspace_allowed);
}

fn bind_at(lease: u64) -> Event {
    Event::BeginBind { owner_id: "entry-1".to_owned(), lease_expires_at_ms: lease }
}

fn confirmed(absence: bool) -> StepRecord {
    StepRecord {
        status: StepStatus::Confirmed,
        evidence_hash: EMPTY.to_owned(),
        actual_absence: absence,
    }
}

fn release_steps(s8_absent: bool, include_s9: bool) -> ReleaseSteps {
    let row = confirmed(false);
    ReleaseSteps {
        s1: Some(row.clone()),
        s2: Some(row.clone()),
        s3: Some(row.clone()),
        s4: Some(row.clone()),
        s5: Some(StepRecord { status: StepStatus::Skipped, evidence_hash: EMPTY.to_owned(), actual_absence: false }),
        s6: Some(row.clone()),
        s7: Some(row.clone()),
        s8: Some(confirmed(s8_absent)),
        s9: if include_s9 { Some(row.clone()) } else { None },
        s10: Some(row.clone()),
        s11: Some(row),
    }
}

#[test]
fn reason_names_and_bounds() {
    assert_eq!(IDLE_BOUNDARY_MS, 1_800_000);
    assert_eq!(ABSOLUTE_BOUNDARY_MS, 28_800_000);
    assert_eq!(TURN_BOUNDARY_MS, 600_000);
    assert_eq!(DEDUP_JOURNAL_CAP, 1024);
    assert_eq!(serde_json::to_value(Reason::Stale).unwrap(), "stale");
    assert_eq!(serde_json::to_value(Reason::S8NotAbsent).unwrap(), "s8_not_absent");
    assert_eq!(serde_json::to_value(StepId::S10).unwrap(), "s10");
    assert_eq!(serde_json::to_value(Phase::Releasing).unwrap(), "releasing");
}

#[test]
fn happy_path_revisions_are_hand_counted() {
    let identity = alice();
    let bound = call(&requested(), &identity, 0, 0, "e1", 100, bind_at(30_100));
    assert_eq!(bound.outcome, Outcome::Applied);
    assert_eq!(bound.revision, 1);
    assert_eq!(bound.owner_generation, 1);
    assert_eq!(bound.phase, Phase::Binding);
    assert_eq!(bound.state.bound, Some(identity.clone()));
    assert!(!bound.physical_release_certified);

    let ready = call(
        &bound.state,
        &identity,
        1,
        1,
        "e2",
        200,
        Event::CompleteBind { workspace_id: "ws-1".to_owned() },
    );
    assert_eq!(ready.revision, 2);
    assert_eq!(ready.phase, Phase::Ready);
    assert_eq!(ready.state.workspace_id.as_deref(), Some("ws-1"));
    assert!(ready.same_workspace_allowed);

    let running = call(
        &ready.state,
        &identity,
        2,
        1,
        "e3",
        300,
        Event::BeginTurn { turn_id: "t1".to_owned() },
    );
    assert_eq!(running.revision, 3);
    assert_eq!(running.phase, Phase::Running);
    assert_eq!(running.state.turn.as_ref().map(|turn| turn.started_at_ms), Some(300));

    let started = call(
        &running.state,
        &identity,
        3,
        1,
        "e4",
        400,
        Event::ToolStart { turn_id: "t1".to_owned(), tool_id: "tool-00".to_owned() },
    );
    assert_eq!(started.revision, 4);
    assert_eq!(started.state.tool_starts, 1);

    let finished_tool = call(
        &started.state,
        &identity,
        4,
        1,
        "e5",
        500,
        Event::ToolFinish { turn_id: "t1".to_owned(), tool_id: "tool-00".to_owned() },
    );
    assert_eq!(finished_tool.revision, 5);
    assert!(finished_tool.state.open_tool_ids.is_empty());

    let idle = call(
        &finished_tool.state,
        &identity,
        5,
        1,
        "e6",
        600,
        Event::FinishTurn { turn_id: "t1".to_owned() },
    );
    assert_eq!(idle.revision, 6);
    assert_eq!(idle.phase, Phase::Idle);
    assert_eq!(idle.state.last_turn_outcome, Some(LastTurnOutcome::Finished));
    assert_eq!(idle.state.revision, 6);
}

#[test]
fn stale_revision_and_generation_leave_the_snapshot() {
    let state = requested();
    let out = call(&state, &alice(), 1, 0, "e-stale", 100, bind_at(30_100));
    assert_same(&out, &state, 0, Reason::Stale);

    let ready = ready_at(4, 2, 0, 0, 50_000, 0);
    let out = call(
        &ready,
        &alice(),
        4,
        1,
        "e-gen",
        10,
        Event::BeginTurn { turn_id: "t1".to_owned() },
    );
    assert_same(&out, &ready, 4, Reason::Stale);
    assert_eq!(out.owner_generation, 2);
}

#[test]
fn identical_replay_is_idempotent_and_conflict_refuses() {
    let identity = alice();
    let event = bind_at(31_000);
    let first = call(&requested(), &identity, 0, 0, "e-bind", 1_000, event.clone());
    assert_eq!(first.revision, 1);
    let replay = call(&first.state, &identity, 0, 0, "e-bind", 1_000, event.clone());
    assert_eq!(replay.outcome, Outcome::Idempotent);
    assert_eq!(replay.reason, None);
    assert_eq!(replay.revision, 1);
    assert_eq!(replay.state, first.state);

    let backward = call(&first.state, &identity, 1, 1, "e-bind", 999, event.clone());
    assert_same(&backward, &first.state, 1, Reason::TimeBackward);

    let conflict = call(
        &first.state,
        &identity,
        1,
        1,
        "e-bind",
        1_000,
        Event::BeginBind { owner_id: "entry-2".to_owned(), lease_expires_at_ms: 31_000 },
    );
    assert_same(&conflict, &first.state, 1, Reason::EventIdConflict);

    let mut bob = alice();
    bob.principal_id = "bob".to_owned();
    let other_tuple = call(&first.state, &bob, 1, 1, "e-bind", 1_000, event);
    assert_same(&other_tuple, &first.state, 1, Reason::EventIdConflict);
}

#[test]
fn changed_actor_beats_stale_and_keeps_workspace() {
    let state = ready_at(3, 1, 0, 0, 5_000_000, 0);
    let mut variants = Vec::new();
    let mut principal = alice();
    principal.principal_id = "bob".to_owned();
    variants.push(principal);
    let mut project = alice();
    project.project_id = "project-2".to_owned();
    variants.push(project);
    let mut role = alice();
    role.role = "admin".to_owned();
    variants.push(role);
    let mut claims = alice();
    claims.claims_hash = EMPTY.to_owned();
    variants.push(claims);
    let mut policy = alice();
    policy.policy_version = "pol-2".to_owned();
    variants.push(policy);
    let mut knowledge = alice();
    knowledge.knowledge_version = "know-2".to_owned();
    variants.push(knowledge);
    let mut config = alice();
    config.config_version = "cfg-2".to_owned();
    variants.push(config);
    for identity in variants {
        let out = call(
            &state,
            &identity,
            99,
            8,
            "e-other",
            10,
            Event::BeginTurn { turn_id: "t1".to_owned() },
        );
        assert_same(&out, &state, 3, Reason::NewSessionRequired);
        assert!(out.new_session_required);
        assert_eq!(out.state.workspace_id.as_deref(), Some("ws-1"));
    }
}

#[test]
fn time_boundaries_refuse_at_the_exact_instant() {
    let idle_state = ready_at(5, 1, 0, 0, 1_830_000, 0);
    let idle = call(
        &idle_state,
        &alice(),
        5,
        1,
        "e-idle",
        IDLE_BOUNDARY_MS,
        Event::BeginTurn { turn_id: "t1".to_owned() },
    );
    assert_same(&idle, &idle_state, 5, Reason::IdleBoundary);

    let just = ready_at(5, 1, 0, 0, 1_829_999, 0);
    let admitted = call(
        &just,
        &alice(),
        5,
        1,
        "e-idle-ok",
        IDLE_BOUNDARY_MS - 1,
        Event::BeginTurn { turn_id: "t1".to_owned() },
    );
    assert_eq!(admitted.outcome, Outcome::Applied);
    assert_eq!(admitted.revision, 6);
    assert_eq!(admitted.phase, Phase::Running);

    let absolute_state = ready_at(8, 1, ABSOLUTE_BOUNDARY_MS, 0, ABSOLUTE_BOUNDARY_MS + 30_000, 0);
    let absolute = call(
        &absolute_state,
        &alice(),
        8,
        1,
        "e-abs",
        ABSOLUTE_BOUNDARY_MS,
        Event::BeginTurn { turn_id: "t1".to_owned() },
    );
    assert_same(&absolute, &absolute_state, 8, Reason::AbsoluteBoundary);

    let absolute_ok = ready_at(8, 1, ABSOLUTE_BOUNDARY_MS - 1, 0, ABSOLUTE_BOUNDARY_MS - 1 + 30_000, 0);
    let admitted = call(
        &absolute_ok,
        &alice(),
        8,
        1,
        "e-abs-ok",
        ABSOLUTE_BOUNDARY_MS - 1,
        Event::BeginTurn { turn_id: "t1".to_owned() },
    );
    assert_eq!(admitted.revision, 9);
    assert_eq!(admitted.phase, Phase::Running);

    let turn_state = running_with(0, 0, TURN_BOUNDARY_MS + 30_000);
    let turn = call(
        &turn_state,
        &alice(),
        10,
        1,
        "e-turn",
        TURN_BOUNDARY_MS,
        Event::ToolStart { turn_id: "t1".to_owned(), tool_id: "tool-00".to_owned() },
    );
    assert_same(&turn, &turn_state, 10, Reason::TurnBoundary);

    let mut early = running_with(0, 0, TURN_BOUNDARY_MS - 1 + 30_000);
    early.revision = 10;
    let admitted = call(
        &early,
        &alice(),
        10,
        1,
        "e-turn-ok",
        TURN_BOUNDARY_MS - 1,
        Event::ToolStart { turn_id: "t1".to_owned(), tool_id: "tool-00".to_owned() },
    );
    assert_eq!(admitted.revision, 11);
    assert_eq!(admitted.state.tool_starts, 1);
}

#[test]
fn tool_sixty_is_the_last_admitted_start() {
    let state = running_with(59, 0, 40_000);
    let sixtieth = call(
        &state,
        &alice(),
        10,
        1,
        "e60",
        10,
        Event::ToolStart { turn_id: "t1".to_owned(), tool_id: "tool-59".to_owned() },
    );
    assert_eq!(sixtieth.outcome, Outcome::Applied);
    assert_eq!(sixtieth.revision, 11);
    assert_eq!(sixtieth.state.tool_starts, 60);
    let refused = call(
        &sixtieth.state,
        &alice(),
        11,
        1,
        "e61",
        11,
        Event::ToolStart { turn_id: "t1".to_owned(), tool_id: "tool-60".to_owned() },
    );
    assert_same(&refused, &sixtieth.state, 11, Reason::ToolBoundary);
    assert_eq!(refused.state.tool_starts, 60);
}

#[test]
fn overflow_does_not_wrap() {
    let mut running = running_with(0, 0, 0);
    running.owner_generation = u64::MAX;
    running.lease_expires_at_ms = 0;
    let takeover = call(
        &running,
        &alice(),
        10,
        u64::MAX,
        "e-gen",
        0,
        Event::Takeover { owner_id: "reaper-1".to_owned(), lease_expires_at_ms: 30_000 },
    );
    assert_same(&takeover, &running, 10, Reason::GenerationOverflow);
    assert_eq!(takeover.owner_generation, u64::MAX);
    assert_eq!(takeover.phase, Phase::Running);

    let mut ready = ready_at(u64::MAX, 1, 0, 0, 50_000, 0);
    ready.revision = u64::MAX;
    let heartbeat = call(
        &ready,
        &alice(),
        u64::MAX,
        1,
        "e-rev",
        10_000,
        Event::Heartbeat { owner_id: "entry-1".to_owned(), lease_expires_at_ms: 40_000 },
    );
    assert_same(&heartbeat, &ready, u64::MAX, Reason::RevisionOverflow);
}

#[test]
fn takeover_heartbeat_and_turn_timeout() {
    let mut running = running_with(1, 0, 1_000);
    running.revision = 7;
    running.open_tool_ids = vec!["tool-00".to_owned()];
    let early = call(
        &running,
        &alice(),
        7,
        1,
        "e-early",
        999,
        Event::Takeover { owner_id: "reaper-1".to_owned(), lease_expires_at_ms: 30_999 },
    );
    assert_same(&early, &running, 7, Reason::LeaseHeld);

    let taken = call(
        &running,
        &alice(),
        7,
        1,
        "e-take",
        1_000,
        Event::Takeover { owner_id: "reaper-1".to_owned(), lease_expires_at_ms: 31_000 },
    );
    assert_eq!(taken.revision, 8);
    assert_eq!(taken.owner_generation, 2);
    assert_eq!(taken.phase, Phase::Releasing);
    assert_eq!(taken.state.failure, Some(FailureCode::OwnerLost));
    assert!(taken.state.turn.is_none());
    assert!(taken.state.open_tool_ids.is_empty());

    let ready = ready_at(2, 1, 0, 0, 50_000, 0);
    let beat = call(
        &ready,
        &alice(),
        2,
        1,
        "e-beat",
        1_000,
        Event::Heartbeat { owner_id: "entry-1".to_owned(), lease_expires_at_ms: 31_000 },
    );
    assert_eq!(beat.revision, 3);
    assert_eq!(beat.state.last_activity_at_ms, 0);
    assert_eq!(beat.state.lease_expires_at_ms, 31_000);

    let mut timed = running_with(1, 0, 1);
    timed.open_tool_ids = vec!["tool-00".to_owned()];
    timed.revision = 4;
    let timeout = call(
        &timed,
        &alice(),
        4,
        1,
        "e-timeout",
        TURN_BOUNDARY_MS,
        Event::TurnTimeout { turn_id: "t1".to_owned() },
    );
    assert_eq!(timeout.revision, 5);
    assert_eq!(timeout.phase, Phase::Idle);
    assert_eq!(timeout.state.failure, None);
    assert_eq!(timeout.state.last_turn_outcome, Some(LastTurnOutcome::TimedOut));
    assert!(timeout.state.open_tool_ids.is_empty());
    assert_ne!(timeout.phase, Phase::Releasing);
}

#[test]
fn releasing_stops_admission_and_close_needs_s8_absence() {
    let state = releasing(4);
    let admitted = call(
        &state,
        &alice(),
        4,
        1,
        "e-nope",
        10,
        Event::BeginTurn { turn_id: "t1".to_owned() },
    );
    assert_same(&admitted, &state, 4, Reason::AdmissionStopped);

    let missing = call(
        &state,
        &alice(),
        4,
        1,
        "e-miss",
        20,
        Event::CompleteRelease { steps: Box::new(release_steps(true, false)) },
    );
    assert_same(&missing, &state, 4, Reason::ReleaseIncomplete);

    let present = call(
        &state,
        &alice(),
        4,
        1,
        "e-false",
        20,
        Event::CompleteRelease { steps: Box::new(release_steps(false, true)) },
    );
    assert_same(&present, &state, 4, Reason::S8NotAbsent);

    let closed = call(
        &state,
        &alice(),
        4,
        1,
        "e-close",
        30,
        Event::CompleteRelease { steps: Box::new(release_steps(true, true)) },
    );
    assert_eq!(closed.outcome, Outcome::Applied);
    assert_eq!(closed.revision, 5);
    assert_eq!(closed.phase, Phase::Closed);
    assert_eq!(closed.state.revision, 5);
    assert!(!closed.physical_release_certified);
    assert!(!closed.database_used);
    assert!(!closed.auth_provider_used);
    assert!(!closed.runtime_used);
    assert!(!closed.same_workspace_allowed);
    assert_eq!(closed.state.release.get(StepId::S8).map(|step| step.actual_absence), Some(true));
    assert_eq!(closed.state.failure, Some(FailureCode::UserCancel));

    let recorded = call(
        &state,
        &alice(),
        4,
        1,
        "e-s8",
        15,
        Event::RecordReleaseStep {
            step: StepId::S8,
            status: StepStatus::Confirmed,
            evidence_hash: EMPTY.to_owned(),
            actual_absence: false,
        },
    );
    assert_eq!(recorded.revision, 5);
    let mismatch = call(
        &recorded.state,
        &alice(),
        5,
        1,
        "e-mix",
        16,
        Event::CompleteRelease { steps: Box::new(release_steps(true, true)) },
    );
    assert_same(&mismatch, &recorded.state, 5, Reason::ReleaseMismatch);
}

#[test]
fn cancel_then_begin_turn_is_stopped() {
    let running = running_with(0, 0, 40_000);
    let cancelled = call(
        &running,
        &alice(),
        10,
        1,
        "e-cancel",
        20,
        Event::Cancel { trigger: CancelTrigger::User },
    );
    assert_eq!(cancelled.revision, 11);
    assert_eq!(cancelled.phase, Phase::Releasing);
    assert_eq!(cancelled.state.failure, Some(FailureCode::UserCancel));
    let again = call(
        &cancelled.state,
        &alice(),
        11,
        1,
        "e-after",
        21,
        Event::BeginTurn { turn_id: "t9".to_owned() },
    );
    assert_same(&again, &cancelled.state, 11, Reason::AdmissionStopped);
}

#[test]
fn full_journal_does_not_drop_and_still_replays() {
    let identity = alice();
    let mut current = call(&requested(), &identity, 0, 0, "e0", 0, bind_at(30_000));
    assert_eq!(current.revision, 1);
    for index in 1..DEDUP_JOURNAL_CAP {
        let event_id = format!("h{index:04}");
        current = call(
            &current.state,
            &identity,
            current.revision,
            1,
            &event_id,
            0,
            Event::Heartbeat { owner_id: "entry-1".to_owned(), lease_expires_at_ms: 30_000 },
        );
        assert_eq!(current.outcome, Outcome::Applied);
        assert_eq!(current.revision, u64::try_from(index).unwrap() + 1);
    }
    assert_eq!(current.state.journal.len(), DEDUP_JOURNAL_CAP);
    assert_eq!(current.revision, 1024);
    assert_eq!(current.state.journal.first().map(|entry| entry.event_id.as_str()), Some("e0"));
    let last_id = current.state.journal.last().map(|entry| entry.event_id.clone());
    let before = current.state.clone();
    let extra = call(
        &before,
        &identity,
        1024,
        1,
        "h-extra",
        0,
        Event::Heartbeat { owner_id: "entry-1".to_owned(), lease_expires_at_ms: 30_000 },
    );
    assert_same(&extra, &before, 1024, Reason::DedupExhausted);
    assert_eq!(extra.state.journal.len(), 1024);
    assert_eq!(extra.state.journal.first().map(|entry| entry.event_id.as_str()), Some("e0"));
    assert_eq!(extra.state.journal.last().map(|entry| entry.event_id.clone()), last_id);

    let replay = call(&before, &identity, 0, 1, "e0", 0, bind_at(30_000));
    assert_eq!(replay.outcome, Outcome::Idempotent);
    assert_eq!(replay.revision, 1024);
    assert_eq!(replay.state.journal.len(), 1024);

    let mut oversized = before;
    oversized.journal.push(JournalEntry { event_id: "overflow-id".to_owned(), canonical_sha256: EMPTY.to_owned() });
    let invalid = call(&oversized, &identity, 1024, 1, "h-more", 0, bind_at(30_000));
    assert_eq!(invalid.reason, Some(Reason::StateInvalid));
    assert_eq!(invalid.state.journal.len(), 1025);
    assert_eq!(invalid.state, oversized);
}
