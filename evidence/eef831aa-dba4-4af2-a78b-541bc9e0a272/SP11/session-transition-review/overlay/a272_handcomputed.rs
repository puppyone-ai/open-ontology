//! Independent hand-counted cases for the frozen session-transition reducer.
//! Expected revisions, generations, reasons, and SHA-256 digests are literals.

#![forbid(unsafe_code)]

#[path = "../src/sha256.rs"]
mod product_sha;

use std::process::Command;

use session_transition::{
    transition, CancelTrigger, Event, FailureCode, IdentityTuple, JournalEntry, Outcome, Phase, Reason,
    ReleaseRecord, ReleaseSteps, SessionState, StepId, StepRecord, StepStatus, TransitionInput,
    TransitionOutput, TurnState, CONTRACT, DEDUP_JOURNAL_CAP, STDIN_MAX_BYTES,
};

const CLAIMS: &str = "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad";
const EMPTY_SHA: &str = "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855";
const ABC_SHA: &str = "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad";
const NIST_448: &str = "248d6a61d20638b8e5c026930c3e6039a33ce45964ff2167f6ecedd419db06c1";
const NIST_896: &str = "cf5b16a778af8380036ce59e7b0492370b249b11e8f07a51afac45037afee9d1";
const NIST_MILLION_A: &str = "cdc76e5c9914fb9281a1c7e284d73e67f1809a48a497200e046d39ccc7112cd0";
const BIND_PREIMAGE: &str = concat!(
    r#"{"event_id":"e-bind","identity":{"project_id":"project-1","principal_id":"alice","role":"analyst","claims_hash":""#,
    "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad",
    r#"","policy_version":"pol-1","knowledge_version":"know-1","config_version":"cfg-1"},"event":{"type":"begin_bind","owner_id":"entry-1","lease_expires_at_ms":31000}}"#,
);
const BIND_SHA: &str = "a0bab5c228e31c54a6bb0b10cb29666a891fc089ef098fe344b3640866f16a27";

fn identity() -> IdentityTuple {
    IdentityTuple {
        project_id: "project-1".to_owned(),
        principal_id: "alice".to_owned(),
        role: "analyst".to_owned(),
        claims_hash: CLAIMS.to_owned(),
        policy_version: "pol-1".to_owned(),
        knowledge_version: "know-1".to_owned(),
        config_version: "cfg-1".to_owned(),
    }
}

fn blank() -> SessionState {
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

fn ready(revision: u64, generation: u64, started: u64, activity: u64, last_now: u64, lease: u64) -> SessionState {
    let mut state = blank();
    state.phase = Phase::Ready;
    state.revision = revision;
    state.owner_id = "entry-1".to_owned();
    state.owner_generation = generation;
    state.lease_expires_at_ms = lease;
    state.bound = Some(identity());
    state.workspace_id = Some("ws-1".to_owned());
    state.session_started_at_ms = started;
    state.last_activity_at_ms = activity;
    state.last_now_ms = last_now;
    state
}

fn running(revision: u64, lease: u64) -> SessionState {
    let mut state = ready(revision, 2, 0, 0, 0, lease);
    state.phase = Phase::Running;
    state.turn = Some(TurnState { turn_id: "turn-1".to_owned(), started_at_ms: 0 });
    state
}

fn binding(revision: u64, generation: u64, lease: u64, last_now: u64) -> SessionState {
    let mut state = ready(revision, generation, 0, 0, last_now, lease);
    state.phase = Phase::Binding;
    state.workspace_id = None;
    state
}

fn releasing(revision: u64, generation: u64, journal: Vec<JournalEntry>) -> SessionState {
    let mut state = ready(revision, generation, 1_000, 2_000, 5_000, 40_000);
    state.phase = Phase::Releasing;
    state.failure = Some(FailureCode::UserCancel);
    state.journal = journal;
    state
}

fn filler(count: usize) -> Vec<JournalEntry> {
    (0..count)
        .map(|index| JournalEntry {
            event_id: format!("j{index:04}"),
            canonical_sha256: EMPTY_SHA.to_owned(),
        })
        .collect()
}

fn confirmed(absence: bool) -> StepRecord {
    StepRecord {
        status: StepStatus::Confirmed,
        evidence_hash: EMPTY_SHA.to_owned(),
        actual_absence: absence,
    }
}

fn release_steps(s8_absent: bool) -> ReleaseSteps {
    let ordinary = Some(confirmed(false));
    ReleaseSteps {
        s1: ordinary.clone(),
        s2: ordinary.clone(),
        s3: ordinary.clone(),
        s4: ordinary.clone(),
        s5: Some(StepRecord {
            status: StepStatus::Skipped,
            evidence_hash: EMPTY_SHA.to_owned(),
            actual_absence: false,
        }),
        s6: ordinary.clone(),
        s7: ordinary.clone(),
        s8: Some(confirmed(s8_absent)),
        s9: ordinary.clone(),
        s10: ordinary.clone(),
        s11: ordinary,
    }
}

fn call(
    state: &SessionState,
    who: &IdentityTuple,
    expected_revision: u64,
    expected_generation: u64,
    event_id: &str,
    now_ms: u64,
    event: Event,
) -> TransitionOutput {
    transition(&TransitionInput {
        contract: CONTRACT.to_owned(),
        state: state.clone(),
        identity: who.clone(),
        expected_revision,
        expected_owner_generation: expected_generation,
        event_id: event_id.to_owned(),
        now_ms,
        event,
    })
}

fn flags(out: &TransitionOutput) {
    assert!(!out.physical_release_certified);
    assert!(!out.database_used);
    assert!(!out.auth_provider_used);
    assert!(!out.runtime_used);
    assert_eq!(out.contract, CONTRACT);
}

fn unchanged(out: &TransitionOutput, before: &SessionState, revision: u64, reason: Reason) {
    assert_eq!(out.outcome, Outcome::Refused);
    assert_eq!(out.reason, Some(reason));
    assert_eq!(out.revision, revision);
    assert_eq!(out.owner_generation, before.owner_generation);
    assert_eq!(&out.state, before);
    flags(out);
}

fn digest(bytes: &[u8]) -> String {
    let hash = product_sha::sha256(bytes).expect("sha256");
    product_sha::hex(&hash)
}

#[test]
fn sha256_known_vectors() {
    assert_eq!(digest(b""), EMPTY_SHA);
    assert_eq!(digest(b"abc"), ABC_SHA);
    assert_eq!(digest(b"abcdbcdecdefdefgefghfghighijhijkijkljklmklmnlmnomnopnopq"), NIST_448);
    assert_eq!(
        digest(b"abcdefghbcdefghicdefghijdefghijkefghijklfghijklmghijklmnhijklmnoijklmnopjklmnopqklmnopqrlmnopqrsmnopqrstnopqrstu"),
        NIST_896
    );
    assert_eq!(digest(&vec![b'a'; 1_000_000]), NIST_MILLION_A);
    assert_eq!(digest(BIND_PREIMAGE.as_bytes()), BIND_SHA);
}

#[test]
fn supplied_revision_and_generation_are_the_cas() {
    let who = identity();
    let begin = Event::BeginBind { owner_id: "entry-1".to_owned(), lease_expires_at_ms: 31_000 };
    let first = call(&blank(), &who, 0, 0, "e-bind", 1_000, begin.clone());
    let again = call(&blank(), &who, 0, 0, "e-bind", 1_000, begin);
    assert_eq!(first, again);
    assert_eq!(first.outcome, Outcome::Applied);
    assert_eq!(first.revision, 1);
    assert_eq!(first.owner_generation, 1);
    assert_eq!(first.phase, Phase::Binding);
    assert_eq!(first.state.revision, 1);
    assert_eq!(first.state.owner_generation, 1);
    assert_eq!(first.state.workspace_id, None);
    assert_eq!(first.state.last_activity_at_ms, 1_000);
    assert_eq!(first.state.journal.len(), 1);
    assert_eq!(first.state.journal[0].canonical_sha256, BIND_SHA);
    flags(&first);
    assert!(!first.database_used);

    let stale = call(
        &first.state,
        &who,
        0,
        1,
        "e-workspace",
        2_000,
        Event::CompleteBind { workspace_id: "ws-1".to_owned() },
    );
    unchanged(&stale, &first.state, 1, Reason::Stale);

    let ready = call(
        &first.state,
        &who,
        1,
        1,
        "e-workspace",
        2_000,
        Event::CompleteBind { workspace_id: "ws-1".to_owned() },
    );
    assert_eq!(ready.outcome, Outcome::Applied);
    assert_eq!(ready.revision, 2);
    assert_eq!(ready.owner_generation, 1);
    assert_eq!(ready.phase, Phase::Ready);
    assert_eq!(ready.state.workspace_id.as_deref(), Some("ws-1"));
    assert_eq!(ready.state.last_activity_at_ms, 2_000);
    assert!(ready.same_workspace_allowed);
    flags(&ready);

    let taken = call(
        &ready.state,
        &who,
        2,
        1,
        "e-take",
        31_000,
        Event::Takeover { owner_id: "entry-2".to_owned(), lease_expires_at_ms: 61_000 },
    );
    assert_eq!(taken.outcome, Outcome::Applied);
    assert_eq!(taken.revision, 3);
    assert_eq!(taken.owner_generation, 2);
    assert_eq!(taken.phase, Phase::Releasing);
    assert_eq!(taken.state.failure, Some(FailureCode::OwnerLost));
    assert_eq!(taken.state.owner_id, "entry-2");
    assert_eq!(taken.state.workspace_id.as_deref(), Some("ws-1"));
    assert_eq!(taken.state.last_activity_at_ms, 2_000);
    assert!(!taken.same_workspace_allowed);
    flags(&taken);

    let stale_gen = call(
        &taken.state,
        &who,
        3,
        1,
        "e-stale-gen",
        31_000,
        Event::Takeover { owner_id: "entry-9".to_owned(), lease_expires_at_ms: 61_000 },
    );
    unchanged(&stale_gen, &taken.state, 3, Reason::Stale);
    assert_eq!(stale_gen.owner_generation, 2);

    let taken_again = call(
        &taken.state,
        &who,
        3,
        2,
        "e-take-2",
        61_000,
        Event::Takeover { owner_id: "entry-3".to_owned(), lease_expires_at_ms: 91_000 },
    );
    assert_eq!(taken_again.outcome, Outcome::Applied);
    assert_eq!(taken_again.revision, 4);
    assert_eq!(taken_again.owner_generation, 3);
    assert_eq!(taken_again.phase, Phase::Releasing);
    assert_eq!(taken_again.state.failure, Some(FailureCode::OwnerLost));
    assert_eq!(taken_again.state.last_activity_at_ms, 2_000);
    flags(&taken_again);
}

#[test]
fn identity_is_not_authentication() {
    let who = identity();
    let state = ready(2, 1, 1_000, 2_000, 2_000, 32_000);
    let mut other = who.clone();
    other.role = "observer".to_owned();
    let changed = call(
        &state,
        &other,
        0,
        9,
        "e-other-role",
        2_100,
        Event::BeginTurn { turn_id: "turn-9".to_owned() },
    );
    unchanged(&changed, &state, 2, Reason::NewSessionRequired);
    assert!(changed.new_session_required);
    assert!(!changed.same_workspace_allowed);
    assert_eq!(changed.state.workspace_id.as_deref(), Some("ws-1"));
    assert_ne!(changed.reason, Some(Reason::Stale));

    let mut unsigned = who.clone();
    unsigned.principal_id = "not-a-directory-user".to_owned();
    unsigned.claims_hash = "0".repeat(64);
    let bound = call(
        &blank(),
        &unsigned,
        0,
        0,
        "e-unsigned",
        1_000,
        Event::BeginBind { owner_id: "entry-1".to_owned(), lease_expires_at_ms: 31_000 },
    );
    assert_eq!(bound.outcome, Outcome::Applied);
    assert_eq!(bound.revision, 1);
    assert_eq!(bound.state.bound.as_ref().map(|item| item.claims_hash.as_str()), Some(unsigned.claims_hash.as_str()));
    assert!(!bound.auth_provider_used);
    flags(&bound);

    let mut value = serde_json::to_value(TransitionInput {
        contract: CONTRACT.to_owned(),
        state: blank(),
        identity: who,
        expected_revision: 0,
        expected_owner_generation: 0,
        event_id: "e-sig".to_owned(),
        now_ms: 1_000,
        event: Event::BeginBind { owner_id: "entry-1".to_owned(), lease_expires_at_ms: 31_000 },
    })
    .expect("value");
    value["identity"]["signature"] = serde_json::Value::String("not-a-check".to_owned());
    assert!(serde_json::from_value::<TransitionInput>(value).is_err());
}

#[test]
fn time_boundaries_and_lease_instant() {
    let who = identity();
    let idle_state = ready(4, 2, 0, 0, 0, 1_830_000);
    let admitted = call(&idle_state, &who, 4, 2, "e-idle-ok", 1_799_999, Event::BeginTurn { turn_id: "turn-1".to_owned() });
    assert_eq!(admitted.outcome, Outcome::Applied);
    assert_eq!(admitted.revision, 5);
    assert_eq!(admitted.phase, Phase::Running);
    assert_eq!(admitted.state.turn.as_ref().map(|turn| turn.started_at_ms), Some(1_799_999));
    flags(&admitted);
    let idle_hit = call(&idle_state, &who, 4, 2, "e-idle-no", 1_800_000, Event::BeginTurn { turn_id: "turn-1".to_owned() });
    unchanged(&idle_hit, &idle_state, 4, Reason::IdleBoundary);

    let absolute_state = ready(4, 2, 0, 28_799_000, 28_799_000, 28_830_000);
    let absolute_hit = call(
        &absolute_state,
        &who,
        4,
        2,
        "e-abs-no",
        28_800_000,
        Event::BeginTurn { turn_id: "turn-1".to_owned() },
    );
    unchanged(&absolute_hit, &absolute_state, 4, Reason::AbsoluteBoundary);
    let absolute_ok = call(
        &absolute_state,
        &who,
        4,
        2,
        "e-abs-ok",
        28_799_999,
        Event::BeginTurn { turn_id: "turn-1".to_owned() },
    );
    assert_eq!(absolute_ok.outcome, Outcome::Applied);
    assert_eq!(absolute_ok.revision, 5);
    flags(&absolute_ok);

    let turn_state = running(6, 630_000);
    let turn_ok = call(
        &turn_state,
        &who,
        6,
        2,
        "e-tool-ok",
        599_999,
        Event::ToolStart { turn_id: "turn-1".to_owned(), tool_id: "tool-1".to_owned() },
    );
    assert_eq!(turn_ok.outcome, Outcome::Applied);
    assert_eq!(turn_ok.revision, 7);
    assert_eq!(turn_ok.state.tool_starts, 1);
    flags(&turn_ok);
    let turn_hit = call(
        &turn_state,
        &who,
        6,
        2,
        "e-tool-no",
        600_000,
        Event::ToolStart { turn_id: "turn-1".to_owned(), tool_id: "tool-1".to_owned() },
    );
    unchanged(&turn_hit, &turn_state, 6, Reason::TurnBoundary);

    let live = binding(6, 3, 50_000, 0);
    let beat = call(
        &live,
        &who,
        6,
        3,
        "e-beat",
        49_999,
        Event::Heartbeat { owner_id: "entry-1".to_owned(), lease_expires_at_ms: 79_999 },
    );
    assert_eq!(beat.outcome, Outcome::Applied);
    assert_eq!(beat.revision, 7);
    assert_eq!(beat.state.last_activity_at_ms, 0);
    assert_eq!(beat.state.lease_expires_at_ms, 79_999);
    flags(&beat);
    let held = call(
        &live,
        &who,
        6,
        3,
        "e-held",
        49_999,
        Event::Takeover { owner_id: "entry-2".to_owned(), lease_expires_at_ms: 79_999 },
    );
    unchanged(&held, &live, 6, Reason::LeaseHeld);
    let expired = call(
        &live,
        &who,
        6,
        3,
        "e-expired",
        50_000,
        Event::Heartbeat { owner_id: "entry-1".to_owned(), lease_expires_at_ms: 80_000 },
    );
    unchanged(&expired, &live, 6, Reason::LeaseExpired);
    let takeover = call(
        &live,
        &who,
        6,
        3,
        "e-own",
        50_000,
        Event::Takeover { owner_id: "entry-2".to_owned(), lease_expires_at_ms: 80_000 },
    );
    assert_eq!(takeover.outcome, Outcome::Applied);
    assert_eq!(takeover.revision, 7);
    assert_eq!(takeover.owner_generation, 4);
    assert_eq!(takeover.phase, Phase::Releasing);
    assert_eq!(takeover.state.failure, Some(FailureCode::OwnerLost));
    assert_eq!(takeover.state.last_activity_at_ms, 0);
    flags(&takeover);

    let replay_state = call(
        &blank(),
        &who,
        0,
        0,
        "e-bind",
        1_000,
        Event::BeginBind { owner_id: "entry-1".to_owned(), lease_expires_at_ms: 31_000 },
    )
    .state;
    let backward = call(
        &replay_state,
        &who,
        1,
        1,
        "e-bind",
        999,
        Event::BeginBind { owner_id: "entry-1".to_owned(), lease_expires_at_ms: 31_000 },
    );
    unchanged(&backward, &replay_state, 1, Reason::TimeBackward);
    let equal_now = call(
        &replay_state,
        &who,
        1,
        1,
        "e-equal",
        1_000,
        Event::Heartbeat { owner_id: "entry-1".to_owned(), lease_expires_at_ms: 31_000 },
    );
    assert_eq!(equal_now.outcome, Outcome::Applied);
    assert_eq!(equal_now.revision, 2);
    assert_eq!(equal_now.state.last_activity_at_ms, 1_000);
    assert_eq!(equal_now.state.last_now_ms, 1_000);
    flags(&equal_now);
}

#[test]
fn tool_sixty_then_refusal() {
    let who = identity();
    let mut state = running(8, 40_000);
    for n in 1u32..=60 {
        let before = state.revision;
        let id = format!("t{n:02}");
        let out = call(
            &state,
            &who,
            before,
            2,
            &format!("e-tool-{n:02}"),
            u64::from(n),
            Event::ToolStart { turn_id: "turn-1".to_owned(), tool_id: id },
        );
        assert_eq!(out.outcome, Outcome::Applied, "{n}");
        assert_eq!(out.revision, 8 + u64::from(n));
        assert_eq!(out.state.tool_starts, n);
        assert_eq!(out.state.phase, Phase::Running);
        flags(&out);
        state = out.state;
    }
    assert_eq!(state.revision, 68);
    assert_eq!(state.tool_starts, 60);
    assert_eq!(state.started_tool_ids.len(), 60);
    assert_eq!(state.started_tool_ids.first().map(String::as_str), Some("t01"));
    assert_eq!(state.started_tool_ids.last().map(String::as_str), Some("t60"));
    let refused = call(
        &state,
        &who,
        68,
        2,
        "e-tool-61",
        61,
        Event::ToolStart { turn_id: "turn-1".to_owned(), tool_id: "t61".to_owned() },
    );
    unchanged(&refused, &state, 68, Reason::ToolBoundary);
    assert_eq!(refused.state.tool_starts, 60);
    assert!(!refused.state.started_tool_ids.iter().any(|id| id == "t61"));
}

#[test]
fn dedup_replay_conflict_and_overflow() {
    let who = identity();
    let bound = call(
        &blank(),
        &who,
        0,
        0,
        "e-bind",
        1_000,
        Event::BeginBind { owner_id: "entry-1".to_owned(), lease_expires_at_ms: 31_000 },
    );
    assert_eq!(bound.revision, 1);
    let replay = call(
        &bound.state,
        &who,
        0,
        0,
        "e-bind",
        1_500,
        Event::BeginBind { owner_id: "entry-1".to_owned(), lease_expires_at_ms: 31_000 },
    );
    assert_eq!(replay.outcome, Outcome::Idempotent);
    assert!(replay.reason.is_none());
    assert_eq!(replay.revision, 1);
    assert_eq!(replay.state, bound.state);
    flags(&replay);
    let conflict = call(
        &bound.state,
        &who,
        1,
        1,
        "e-bind",
        1_500,
        Event::BeginBind { owner_id: "entry-2".to_owned(), lease_expires_at_ms: 31_000 },
    );
    unchanged(&conflict, &bound.state, 1, Reason::EventIdConflict);

    let mut full = bound.state.clone();
    full.journal.extend(filler(DEDUP_JOURNAL_CAP - 1));
    assert_eq!(full.journal.len(), DEDUP_JOURNAL_CAP);
    let replay_full = call(
        &full,
        &who,
        0,
        1,
        "e-bind",
        1_500,
        Event::BeginBind { owner_id: "entry-1".to_owned(), lease_expires_at_ms: 31_000 },
    );
    assert_eq!(replay_full.outcome, Outcome::Idempotent);
    assert_eq!(replay_full.revision, 1);
    assert_eq!(replay_full.state.journal.len(), DEDUP_JOURNAL_CAP);
    let exhausted = call(
        &full,
        &who,
        1,
        1,
        "e-new",
        1_500,
        Event::Heartbeat { owner_id: "entry-1".to_owned(), lease_expires_at_ms: 31_000 },
    );
    unchanged(&exhausted, &full, 1, Reason::DedupExhausted);

    let mut max_gen = ready(4, u64::MAX, 0, 0, 0, 0);
    let overflow = call(
        &max_gen,
        &who,
        4,
        u64::MAX,
        "e-gen",
        0,
        Event::Takeover { owner_id: "entry-2".to_owned(), lease_expires_at_ms: 1_000 },
    );
    unchanged(&overflow, &max_gen, 4, Reason::GenerationOverflow);
    assert_eq!(overflow.state.owner_generation, u64::MAX);
    max_gen.journal = filler(DEDUP_JOURNAL_CAP);
    let hidden = call(
        &max_gen,
        &who,
        4,
        u64::MAX,
        "e-gen-full",
        0,
        Event::Takeover { owner_id: "entry-2".to_owned(), lease_expires_at_ms: 1_000 },
    );
    unchanged(&hidden, &max_gen, 4, Reason::DedupExhausted);

    let max_rev = ready(u64::MAX, 1, 0, 0, 0, 40_000);
    let wrong_owner = call(
        &max_rev,
        &who,
        u64::MAX,
        1,
        "e-owner",
        1,
        Event::Heartbeat { owner_id: "entry-2".to_owned(), lease_expires_at_ms: 30_001 },
    );
    unchanged(&wrong_owner, &max_rev, u64::MAX, Reason::OwnerMismatch);
    let rev = call(
        &max_rev,
        &who,
        u64::MAX,
        1,
        "e-rev",
        1,
        Event::Heartbeat { owner_id: "entry-1".to_owned(), lease_expires_at_ms: 30_001 },
    );
    unchanged(&rev, &max_rev, u64::MAX, Reason::RevisionOverflow);
}

#[test]
fn full_journal_refuses_release() {
    let who = identity();
    let state = releasing(9, 1, filler(DEDUP_JOURNAL_CAP));
    assert_eq!(state.journal.len(), 1024);
    assert_eq!(state.journal.first().map(|entry| entry.event_id.as_str()), Some("j0000"));
    assert_eq!(state.journal.last().map(|entry| entry.event_id.as_str()), Some("j1023"));
    let not_absent = call(
        &state,
        &who,
        9,
        1,
        "e-s8-false",
        5_000,
        Event::CompleteRelease { steps: Box::new(release_steps(false)) },
    );
    unchanged(&not_absent, &state, 9, Reason::S8NotAbsent);
    let release = call(
        &state,
        &who,
        9,
        1,
        "e-release",
        5_000,
        Event::CompleteRelease { steps: Box::new(release_steps(true)) },
    );
    unchanged(&release, &state, 9, Reason::DedupExhausted);
    assert_eq!(release.phase, Phase::Releasing);
    assert_eq!(release.state.journal.len(), 1024);
    assert!(release.state.release.s1.is_none());
    assert!(release.state.release.s8.is_none());
    assert_eq!(release.state.journal.first().map(|entry| entry.event_id.as_str()), Some("j0000"));
    assert_eq!(release.state.journal.last().map(|entry| entry.event_id.as_str()), Some("j1023"));
    assert!(!release.physical_release_certified);
    assert!(!release.database_used);
    assert!(!release.runtime_used);
}

#[test]
fn release_is_caller_declared_and_closed_is_immutable() {
    let who = identity();
    let state = ready(3, 1, 1_000, 2_000, 2_000, 32_000);
    let cancelled = call(
        &state,
        &who,
        3,
        1,
        "e-cancel",
        3_000,
        Event::Cancel { trigger: CancelTrigger::User },
    );
    assert_eq!(cancelled.outcome, Outcome::Applied);
    assert_eq!(cancelled.revision, 4);
    assert_eq!(cancelled.phase, Phase::Releasing);
    assert_eq!(cancelled.state.failure, Some(FailureCode::UserCancel));
    assert_eq!(cancelled.state.last_activity_at_ms, 2_000);
    assert_eq!(cancelled.state.workspace_id.as_deref(), Some("ws-1"));
    flags(&cancelled);
    let blocked = call(
        &cancelled.state,
        &who,
        4,
        1,
        "e-block",
        3_000,
        Event::BeginTurn { turn_id: "turn-1".to_owned() },
    );
    unchanged(&blocked, &cancelled.state, 4, Reason::AdmissionStopped);
    let absent = call(
        &cancelled.state,
        &who,
        4,
        1,
        "e-absent",
        3_001,
        Event::CompleteRelease { steps: Box::new(release_steps(false)) },
    );
    unchanged(&absent, &cancelled.state, 4, Reason::S8NotAbsent);
    let mut bad_s1 = release_steps(true);
    bad_s1.s1 = Some(confirmed(true));
    let only_s8 = call(
        &cancelled.state,
        &who,
        4,
        1,
        "e-only-s8",
        3_001,
        Event::CompleteRelease { steps: Box::new(bad_s1) },
    );
    unchanged(&only_s8, &cancelled.state, 4, Reason::AbsenceOnlyS8);

    let closed = call(
        &cancelled.state,
        &who,
        4,
        1,
        "e-close",
        3_001,
        Event::CompleteRelease { steps: Box::new(release_steps(true)) },
    );
    assert_eq!(closed.outcome, Outcome::Applied);
    assert_eq!(closed.revision, 5);
    assert_eq!(closed.phase, Phase::Closed);
    assert_eq!(closed.state.release.get(StepId::S8).map(|step| step.actual_absence), Some(true));
    assert_eq!(closed.state.release.get(StepId::S5).map(|step| step.status), Some(StepStatus::Skipped));
    assert!(!closed.physical_release_certified);
    assert!(!closed.runtime_used);
    assert!(!closed.database_used);
    assert!(!closed.auth_provider_used);
    assert!(!closed.same_workspace_allowed);
    flags(&closed);

    let events = [
        Event::BeginTurn { turn_id: "turn-2".to_owned() },
        Event::Cancel { trigger: CancelTrigger::Admin },
        Event::Heartbeat { owner_id: "entry-1".to_owned(), lease_expires_at_ms: 33_000 },
        Event::Takeover { owner_id: "entry-2".to_owned(), lease_expires_at_ms: 33_000 },
        Event::RecordReleaseStep {
            step: StepId::S8,
            status: StepStatus::Confirmed,
            evidence_hash: EMPTY_SHA.to_owned(),
            actual_absence: true,
        },
        Event::CompleteRelease { steps: Box::new(release_steps(true)) },
        Event::ToolStart { turn_id: "turn-1".to_owned(), tool_id: "tool-1".to_owned() },
    ];
    for (index, event) in events.into_iter().enumerate() {
        let out = call(&closed.state, &who, 5, 1, &format!("e-closed-{index}"), 3_100, event);
        unchanged(&out, &closed.state, 5, Reason::Closed);
    }
    let replay = call(
        &closed.state,
        &who,
        0,
        1,
        "e-close",
        3_200,
        Event::CompleteRelease { steps: Box::new(release_steps(true)) },
    );
    assert_eq!(replay.outcome, Outcome::Idempotent);
    assert_eq!(replay.revision, 5);
    assert_eq!(replay.state, closed.state);
    assert!(!replay.physical_release_certified);
    flags(&replay);
}

#[test]
fn malformed_snapshots_refuse() {
    let who = identity();
    let event = Event::BeginTurn { turn_id: "turn-1".to_owned() };
    let mut oversized = blank();
    oversized.journal = filler(DEDUP_JOURNAL_CAP + 1);
    let over = call(&oversized, &who, 0, 0, "e-over", 1, event.clone());
    unchanged(&over, &oversized, 0, Reason::StateInvalid);

    let mut duplicate = blank();
    duplicate.journal = vec![
        JournalEntry { event_id: "same-id".to_owned(), canonical_sha256: EMPTY_SHA.to_owned() },
        JournalEntry { event_id: "same-id".to_owned(), canonical_sha256: ABC_SHA.to_owned() },
    ];
    let dup = call(&duplicate, &who, 0, 0, "e-dup", 1, event.clone());
    unchanged(&dup, &duplicate, 0, Reason::StateInvalid);

    let mut bad_hash = blank();
    bad_hash.journal = vec![JournalEntry { event_id: "e-bad".to_owned(), canonical_sha256: "AA".to_owned() }];
    let hashed = call(&bad_hash, &who, 0, 0, "e-hash", 1, event.clone());
    unchanged(&hashed, &bad_hash, 0, Reason::StateInvalid);

    let mut owner = blank();
    owner.owner_id = "entry-1".to_owned();
    let owned = call(&owner, &who, 0, 0, "e-owner", 1, event.clone());
    unchanged(&owned, &owner, 0, Reason::StateInvalid);

    let mut tools = running(3, 40_000);
    tools.started_tool_ids = vec!["b".to_owned(), "a".to_owned()];
    tools.tool_starts = 2;
    let unsorted = call(&tools, &who, 3, 2, "e-sort", 1, event.clone());
    unchanged(&unsorted, &tools, 3, Reason::StateInvalid);

    let mut open = running(3, 40_000);
    open.started_tool_ids = vec!["a".to_owned()];
    open.open_tool_ids = vec!["b".to_owned()];
    open.tool_starts = 1;
    let missing = call(&open, &who, 3, 2, "e-open", 1, event.clone());
    unchanged(&missing, &open, 3, Reason::StateInvalid);

    let mut no_turn = running(3, 40_000);
    no_turn.turn = None;
    let shaped = call(&no_turn, &who, 3, 2, "e-shape", 1, event);
    unchanged(&shaped, &no_turn, 3, Reason::StateInvalid);

    let mut closed = releasing(5, 1, Vec::new());
    closed.phase = Phase::Closed;
    let incomplete = call(&closed, &who, 5, 1, "e-closed", 5_000, Event::BeginTurn { turn_id: "turn-1".to_owned() });
    unchanged(&incomplete, &closed, 5, Reason::StateInvalid);

    closed.release = {
        let mut release = ReleaseRecord::default();
        for step in [
            StepId::S1,
            StepId::S2,
            StepId::S3,
            StepId::S4,
            StepId::S5,
            StepId::S6,
            StepId::S7,
            StepId::S8,
            StepId::S9,
            StepId::S10,
            StepId::S11,
        ] {
            release.set(step, confirmed(false));
        }
        release
    };
    let false_s8 = call(&closed, &who, 5, 1, "e-false-s8", 5_000, Event::BeginTurn { turn_id: "turn-1".to_owned() });
    unchanged(&false_s8, &closed, 5, Reason::StateInvalid);
}

#[test]
fn cli_malformed_and_invalid_state() {
    let input = serde_json::to_vec(&TransitionInput {
        contract: CONTRACT.to_owned(),
        state: blank(),
        identity: identity(),
        expected_revision: 0,
        expected_owner_generation: 0,
        event_id: "e-cli".to_owned(),
        now_ms: 1_000,
        event: Event::BeginBind { owner_id: "entry-1".to_owned(), lease_expires_at_ms: 31_000 },
    })
    .expect("json");
    let mut trailing = input.clone();
    trailing.extend_from_slice(b" {}");
    let (code, stdout, stderr) = run_cli(&[], &trailing);
    assert_eq!(code, 1, "{stderr}");
    assert_eq!(stderr, "session-transition: malformed json\n");
    let body: serde_json::Value = serde_json::from_str(stdout.trim()).expect("stdout");
    assert_eq!(body["outcome"], "malformed");
    assert!(body.get("revision").is_none());
    assert_eq!(body["database_used"], false);
    assert_eq!(body["auth_provider_used"], false);
    assert_eq!(body["runtime_used"], false);
    assert_eq!(body["physical_release_certified"], false);

    let (code, stdout, stderr) = run_cli(&["--help"], b"");
    assert_eq!(code, 1, "{stderr}");
    assert_eq!(stderr, "session-transition: malformed usage\n");
    let usage: serde_json::Value = serde_json::from_str(stdout.trim()).expect("usage");
    assert_eq!(usage["reason"], "usage");
    assert!(usage.get("revision").is_none());

    let mut huge = vec![b'a'; STDIN_MAX_BYTES + 1];
    let (code, stdout, stderr) = run_cli(&[], &huge);
    huge.clear();
    assert_eq!(code, 1, "{stderr}");
    assert_eq!(stderr, "session-transition: malformed stdin_too_large\n");
    let large: serde_json::Value = serde_json::from_str(stdout.trim()).expect("large");
    assert!(large.get("revision").is_none());

    let mut bad = blank();
    bad.owner_id = "entry-1".to_owned();
    let bad_input = TransitionInput {
        contract: CONTRACT.to_owned(),
        state: bad,
        identity: identity(),
        expected_revision: 0,
        expected_owner_generation: 0,
        event_id: "e-bad".to_owned(),
        now_ms: 1_000,
        event: Event::BeginBind { owner_id: "entry-1".to_owned(), lease_expires_at_ms: 31_000 },
    };
    let bytes = serde_json::to_vec(&bad_input).expect("bad json");
    let (code, stdout, stderr) = run_cli(&[], &bytes);
    assert_eq!(code, 2, "{stderr}{stdout}");
    assert_eq!(stderr, "session-transition: refused state_invalid\n");
    let refused: serde_json::Value = serde_json::from_str(stdout.trim()).expect("refused");
    assert_eq!(refused["outcome"], "refused");
    assert_eq!(refused["reason"], "state_invalid");
    assert_eq!(refused["revision"], 0);
    assert_eq!(refused["database_used"], false);
    assert_eq!(refused["physical_release_certified"], false);
}

fn executable() -> &'static str {
    if let Some(path) = option_env!("CARGO_BIN_EXE_session-transition") {
        return path;
    }
    if let Some(path) = option_env!("CARGO_BIN_EXE_session_transition") {
        return path;
    }
    panic!("missing session-transition executable path");
}

fn run_cli(args: &[&str], stdin: &[u8]) -> (i32, String, String) {
    let mut child = Command::new(executable());
    child
        .args(args)
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .env_remove("RUST_LOG");
    let mut child = child.spawn().expect("spawn");
    {
        use std::io::Write;
        child.stdin.as_mut().expect("stdin").write_all(stdin).expect("write");
    }
    let output = child.wait_with_output().expect("wait");
    let code = output.status.code().unwrap_or(127);
    let stdout = String::from_utf8(output.stdout).expect("stdout utf8");
    let stderr = String::from_utf8(output.stderr).expect("stderr utf8");
    (code, stdout, stderr)
}
