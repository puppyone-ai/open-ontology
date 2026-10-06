use serde::{Deserialize, Serialize};

pub const CONTRACT: &str = "session-transition-v1";
pub const IDLE_BOUNDARY_MS: u64 = 1_800_000;
pub const ABSOLUTE_BOUNDARY_MS: u64 = 28_800_000;
pub const TURN_BOUNDARY_MS: u64 = 600_000;
pub const TOOL_BOUNDARY: u32 = 60;
pub const LEASE_MAX_MS: u64 = 30_000;
pub const DEDUP_JOURNAL_CAP: usize = 1024;
pub const STDIN_MAX_BYTES: usize = 1_048_576;

pub const STEPS: [StepId; 11] = [
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
];

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Phase {
    Requested,
    Binding,
    Ready,
    Running,
    Idle,
    Releasing,
    Closed,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Outcome {
    Applied,
    Idempotent,
    Refused,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Reason {
    StateInvalid,
    InputInvalid,
    TimeBackward,
    EventIdConflict,
    NewSessionRequired,
    DedupExhausted,
    Stale,
    RevisionOverflow,
    GenerationOverflow,
    LeaseNotFuture,
    LeaseTooLong,
    LeaseExpired,
    LeaseHeld,
    WrongPhase,
    OwnerMismatch,
    IdleBoundary,
    AbsoluteBoundary,
    TurnBoundary,
    ToolBoundary,
    ToolIdReused,
    ToolNotOpen,
    ToolsOpen,
    TurnMismatch,
    TurnNotReached,
    IdleNotReached,
    AbsoluteNotReached,
    NotStarted,
    AlreadyReleasing,
    AdmissionStopped,
    Closed,
    AbsenceOnlyS8,
    StepStatus,
    ReleaseIncomplete,
    S8NotAbsent,
    ReleaseMismatch,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FailureCode {
    UserCancel,
    AdminTerminate,
    EntryLost,
    OwnerLost,
    IdleTimeout,
    AbsoluteTimeout,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum LastTurnOutcome {
    Finished,
    TimedOut,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum StepId {
    #[serde(rename = "s1")]
    S1,
    #[serde(rename = "s2")]
    S2,
    #[serde(rename = "s3")]
    S3,
    #[serde(rename = "s4")]
    S4,
    #[serde(rename = "s5")]
    S5,
    #[serde(rename = "s6")]
    S6,
    #[serde(rename = "s7")]
    S7,
    #[serde(rename = "s8")]
    S8,
    #[serde(rename = "s9")]
    S9,
    #[serde(rename = "s10")]
    S10,
    #[serde(rename = "s11")]
    S11,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum StepStatus {
    Confirmed,
    Skipped,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CancelTrigger {
    User,
    Admin,
    EntryLost,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct StepRecord {
    pub status: StepStatus,
    pub evidence_hash: String,
    pub actual_absence: bool,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(deny_unknown_fields)]
pub struct ReleaseRecord {
    pub s1: Option<StepRecord>,
    pub s2: Option<StepRecord>,
    pub s3: Option<StepRecord>,
    pub s4: Option<StepRecord>,
    pub s5: Option<StepRecord>,
    pub s6: Option<StepRecord>,
    pub s7: Option<StepRecord>,
    pub s8: Option<StepRecord>,
    pub s9: Option<StepRecord>,
    pub s10: Option<StepRecord>,
    pub s11: Option<StepRecord>,
}

impl ReleaseRecord {
    pub fn get(&self, step: StepId) -> Option<&StepRecord> {
        match step {
            StepId::S1 => self.s1.as_ref(),
            StepId::S2 => self.s2.as_ref(),
            StepId::S3 => self.s3.as_ref(),
            StepId::S4 => self.s4.as_ref(),
            StepId::S5 => self.s5.as_ref(),
            StepId::S6 => self.s6.as_ref(),
            StepId::S7 => self.s7.as_ref(),
            StepId::S8 => self.s8.as_ref(),
            StepId::S9 => self.s9.as_ref(),
            StepId::S10 => self.s10.as_ref(),
            StepId::S11 => self.s11.as_ref(),
        }
    }

    pub fn set(&mut self, step: StepId, record: StepRecord) {
        let slot = match step {
            StepId::S1 => &mut self.s1,
            StepId::S2 => &mut self.s2,
            StepId::S3 => &mut self.s3,
            StepId::S4 => &mut self.s4,
            StepId::S5 => &mut self.s5,
            StepId::S6 => &mut self.s6,
            StepId::S7 => &mut self.s7,
            StepId::S8 => &mut self.s8,
            StepId::S9 => &mut self.s9,
            StepId::S10 => &mut self.s10,
            StepId::S11 => &mut self.s11,
        };
        *slot = Some(record);
    }

    pub fn all_empty(&self) -> bool {
        STEPS.iter().all(|step| self.get(*step).is_none())
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ReleaseSteps {
    #[serde(default)]
    pub s1: Option<StepRecord>,
    #[serde(default)]
    pub s2: Option<StepRecord>,
    #[serde(default)]
    pub s3: Option<StepRecord>,
    #[serde(default)]
    pub s4: Option<StepRecord>,
    #[serde(default)]
    pub s5: Option<StepRecord>,
    #[serde(default)]
    pub s6: Option<StepRecord>,
    #[serde(default)]
    pub s7: Option<StepRecord>,
    #[serde(default)]
    pub s8: Option<StepRecord>,
    #[serde(default)]
    pub s9: Option<StepRecord>,
    #[serde(default)]
    pub s10: Option<StepRecord>,
    #[serde(default)]
    pub s11: Option<StepRecord>,
}

impl ReleaseSteps {
    pub fn get(&self, step: StepId) -> Option<&StepRecord> {
        match step {
            StepId::S1 => self.s1.as_ref(),
            StepId::S2 => self.s2.as_ref(),
            StepId::S3 => self.s3.as_ref(),
            StepId::S4 => self.s4.as_ref(),
            StepId::S5 => self.s5.as_ref(),
            StepId::S6 => self.s6.as_ref(),
            StepId::S7 => self.s7.as_ref(),
            StepId::S8 => self.s8.as_ref(),
            StepId::S9 => self.s9.as_ref(),
            StepId::S10 => self.s10.as_ref(),
            StepId::S11 => self.s11.as_ref(),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct IdentityTuple {
    pub project_id: String,
    pub principal_id: String,
    pub role: String,
    pub claims_hash: String,
    pub policy_version: String,
    pub knowledge_version: String,
    pub config_version: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TurnState {
    pub turn_id: String,
    pub started_at_ms: u64,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct JournalEntry {
    pub event_id: String,
    pub canonical_sha256: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SessionState {
    pub session_id: String,
    pub conversation_id: String,
    pub phase: Phase,
    pub revision: u64,
    pub owner_id: String,
    pub owner_generation: u64,
    pub lease_expires_at_ms: u64,
    pub bound: Option<IdentityTuple>,
    pub workspace_id: Option<String>,
    pub session_started_at_ms: u64,
    pub last_activity_at_ms: u64,
    pub last_now_ms: u64,
    pub turn: Option<TurnState>,
    pub tool_starts: u32,
    pub started_tool_ids: Vec<String>,
    pub open_tool_ids: Vec<String>,
    pub last_turn_outcome: Option<LastTurnOutcome>,
    pub failure: Option<FailureCode>,
    pub release: ReleaseRecord,
    pub journal: Vec<JournalEntry>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case", deny_unknown_fields)]
pub enum Event {
    BeginBind {
        owner_id: String,
        lease_expires_at_ms: u64,
    },
    CompleteBind {
        workspace_id: String,
    },
    BeginTurn {
        turn_id: String,
    },
    FinishTurn {
        turn_id: String,
    },
    ToolStart {
        turn_id: String,
        tool_id: String,
    },
    ToolFinish {
        turn_id: String,
        tool_id: String,
    },
    Heartbeat {
        owner_id: String,
        lease_expires_at_ms: u64,
    },
    Takeover {
        owner_id: String,
        lease_expires_at_ms: u64,
    },
    Cancel {
        trigger: CancelTrigger,
    },
    TurnTimeout {
        turn_id: String,
    },
    IdleTimeout,
    AbsoluteTimeout,
    RecordReleaseStep {
        step: StepId,
        status: StepStatus,
        evidence_hash: String,
        actual_absence: bool,
    },
    CompleteRelease {
        steps: Box<ReleaseSteps>,
    },
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TransitionInput {
    pub contract: String,
    pub state: SessionState,
    pub identity: IdentityTuple,
    pub expected_revision: u64,
    pub expected_owner_generation: u64,
    pub event_id: String,
    pub now_ms: u64,
    pub event: Event,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
#[allow(clippy::struct_excessive_bools)]
pub struct TransitionOutput {
    pub contract: String,
    pub outcome: Outcome,
    pub reason: Option<Reason>,
    pub revision: u64,
    pub owner_generation: u64,
    pub phase: Phase,
    pub new_session_required: bool,
    pub same_workspace_allowed: bool,
    pub physical_release_certified: bool,
    pub database_used: bool,
    pub auth_provider_used: bool,
    pub runtime_used: bool,
    pub state: SessionState,
}

pub fn is_token(value: &str) -> bool {
    let bytes = value.as_bytes();
    (1..=128).contains(&bytes.len())
        && bytes.iter().all(|byte| {
            byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'_' | b':' | b'/' | b'-')
        })
}

pub fn is_hex64(value: &str) -> bool {
    value.len() == 64
        && value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
}

pub fn identity_ok(identity: &IdentityTuple) -> bool {
    is_token(&identity.project_id)
        && is_token(&identity.principal_id)
        && is_token(&identity.role)
        && is_hex64(&identity.claims_hash)
        && is_token(&identity.policy_version)
        && is_token(&identity.knowledge_version)
        && is_token(&identity.config_version)
}
