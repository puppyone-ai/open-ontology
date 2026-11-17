//! File state machine (00 §5.1, 10 §4.2). Board WS-BE5.
//!
//! [`is_legal`] is the single transition table. [`transition`] checks it
//! under a row lock and writes the new state, the history row and the
//! `file.state_changed` event in one transaction, in the order 4.1.3 fixes:
//! `file` lock, `file` update, `file_state_history`, `append_event` last.

use sqlx::{PgConnection, PgPool};

use crate::store::StoreError;
use crate::store::events;

/// `_oo_docs.file.state`. `Stored`..`Published` is the main path; the rest
/// are side or lifecycle states.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum FileState {
    /// Listed, fingerprint stable.
    Stored,
    /// Hashed, waiting for parse.
    Queued,
    /// Parse job leased.
    Parsing,
    /// IR in CAS.
    Parsed,
    /// Extract job leased.
    Extracting,
    /// Draft facts stored.
    Indexed,
    /// All publishable facts in some version.
    Published,
    /// Parse gave up or was quarantined.
    ParseFailed,
    /// At least one LLM batch failed after retries.
    ExtractPartial,
    /// Side: format or drive limit.
    Unsupported,
    /// Side: over `max_file_bytes`.
    TooLarge,
    /// Side: encrypted.
    Encrypted,
    /// Side: same blob already processed.
    Duplicate,
    /// Lifecycle: a new blob replaced this version.
    Superseded,
    /// Lifecycle: removed at the source.
    Deleted,
    /// Lifecycle: parser or prompt upgraded, reprocess pending.
    Outdated,
}

impl FileState {
    /// Every state.
    pub const ALL: [FileState; 16] = [
        Self::Stored,
        Self::Queued,
        Self::Parsing,
        Self::Parsed,
        Self::Extracting,
        Self::Indexed,
        Self::Published,
        Self::ParseFailed,
        Self::ExtractPartial,
        Self::Unsupported,
        Self::TooLarge,
        Self::Encrypted,
        Self::Duplicate,
        Self::Superseded,
        Self::Deleted,
        Self::Outdated,
    ];

    /// Database text (the CHECK constraint values).
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Stored => "stored",
            Self::Queued => "queued",
            Self::Parsing => "parsing",
            Self::Parsed => "parsed",
            Self::Extracting => "extracting",
            Self::Indexed => "indexed",
            Self::Published => "published",
            Self::ParseFailed => "parse_failed",
            Self::ExtractPartial => "extract_partial",
            Self::Unsupported => "unsupported",
            Self::TooLarge => "too_large",
            Self::Encrypted => "encrypted",
            Self::Duplicate => "duplicate",
            Self::Superseded => "superseded",
            Self::Deleted => "deleted",
            Self::Outdated => "outdated",
        }
    }

    /// Parse database text. Unknown values are `None`; callers show them
    /// read-only (3.1 rule 5).
    pub fn parse(text: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|s| s.as_str() == text)
    }
}

/// Edges of the 10 §4.2 table other than the "any state" lifecycle rows.
const EDGES: &[(FileState, &[FileState])] = {
    use FileState::*;
    &[
        // Stored -> ParseFailed is the 10 4.3.2 "unreadable file" row
        // (DOCS_SOURCE_PERMISSION_DENIED, retryable): the object could not be
        // read, so the file never reaches `queued`.
        (
            Stored,
            &[TooLarge, Unsupported, Queued, Duplicate, ParseFailed],
        ),
        (Queued, &[Unsupported, Encrypted, Parsing]),
        (Parsing, &[Parsed, Queued, ParseFailed]),
        (Parsed, &[Extracting]),
        (Extracting, &[Indexed, ExtractPartial]),
        (Indexed, &[Published, Outdated]),
        (ExtractPartial, &[Published, Queued, Outdated]),
        (Published, &[Outdated]),
        (ParseFailed, &[Queued, Outdated]),
        (Unsupported, &[Outdated]),
        (Duplicate, &[Outdated]),
        (Outdated, &[Parsing]),
        (Superseded, &[Stored]),
        (Deleted, &[Stored]),
    ]
};

/// Legal transitions (10 §4.2). `from = None` is file creation.
///
/// Reading of the table where the prose is loose:
/// - "any active state -> superseded" and "any -> deleted" cover every state
///   except `Superseded`/`Deleted` themselves; the file then re-enters
///   `Stored` with the new version (`Superseded -> Stored`) or on
///   re-appearance (`Deleted -> Stored`).
/// - "published etc. terminal -> outdated": `Published`, `Indexed`,
///   `ExtractPartial`, `Duplicate`, `ParseFailed`, `Unsupported`.
/// - `Outdated -> Parsing` when the reprocess job is leased.
pub fn is_legal(from: Option<FileState>, to: FileState) -> bool {
    use FileState::*;
    let Some(from) = from else {
        return to == Stored;
    };
    if to == Superseded || to == Deleted {
        return !matches!(from, Superseded | Deleted) || (from == Superseded && to == Deleted);
    }
    EDGES
        .iter()
        .any(|(f, targets)| *f == from && targets.contains(&to))
}

/// States with no outgoing transition other than lifecycle ones.
pub fn is_side(state: FileState) -> bool {
    matches!(
        state,
        FileState::Unsupported | FileState::TooLarge | FileState::Encrypted | FileState::Duplicate
    )
}

/// Why a transition did not happen.
#[derive(Debug)]
pub enum TransitionError {
    /// No such file row.
    NotFound,
    /// The stored state is not the one the caller expected.
    Conflict {
        /// State found under the lock.
        actual: String,
    },
    /// Not in the table.
    Illegal {
        /// From.
        from: FileState,
        /// To.
        to: FileState,
    },
    /// The stored state is not a known value.
    UnknownState(String),
    /// Database failure; the transaction rolled back.
    Store(StoreError),
}

impl std::fmt::Display for TransitionError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::NotFound => f.write_str("file not found"),
            Self::Conflict { actual } => write!(f, "file state is {actual}"),
            Self::Illegal { from, to } => {
                write!(f, "illegal transition {} -> {}", from.as_str(), to.as_str())
            }
            Self::UnknownState(s) => write!(f, "unknown file state {s}"),
            Self::Store(e) => e.fmt(f),
        }
    }
}

impl std::error::Error for TransitionError {}

impl From<sqlx::Error> for TransitionError {
    fn from(e: sqlx::Error) -> Self {
        Self::Store(e.into())
    }
}

impl From<StoreError> for TransitionError {
    fn from(e: StoreError) -> Self {
        Self::Store(e)
    }
}

/// One requested transition.
#[derive(Clone, Debug)]
pub struct Transition<'a> {
    /// File id (UUID text).
    pub file_id: &'a str,
    /// Required current state, or `None` to accept any (still checked
    /// against the table).
    pub expect_from: Option<FileState>,
    /// Target.
    pub to: FileState,
    /// `state_code` (`DOCS_*`) or side reason.
    pub code: Option<&'a str>,
    /// `file.retryable`.
    pub retryable: bool,
    /// Version the change belongs to.
    pub version_id: Option<&'a str>,
    /// Job attempt, recorded in the history.
    pub attempt: Option<i32>,
    /// JSON object text for the history row; no content or credentials.
    pub detail_json: Option<&'a str>,
}

/// Result of a committed transition.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Transitioned {
    /// Previous state.
    pub from: FileState,
    /// New state.
    pub to: FileState,
    /// History sequence for this file.
    pub history_seq: i64,
    /// Event sequence.
    pub event_seq: i64,
}

/// Transition in its own transaction.
pub async fn transition(
    pool: &PgPool,
    req: &Transition<'_>,
) -> Result<Transitioned, TransitionError> {
    let mut tx = pool.begin().await?;
    match transition_in_tx(&mut tx, req).await {
        Ok(done) => {
            tx.commit().await?;
            Ok(done)
        }
        Err(err) => {
            let _ = tx.rollback().await;
            Err(err)
        }
    }
}

/// Transition inside the caller's transaction (so a stage can finish its job
/// and write its products in the same commit). The caller commits.
pub async fn transition_in_tx(
    conn: &mut PgConnection,
    req: &Transition<'_>,
) -> Result<Transitioned, TransitionError> {
    let row: Option<(String, Option<String>)> = sqlx::query_as(
        "SELECT state, drive_id::text FROM _oo_docs.file WHERE id = $1::uuid FOR UPDATE",
    )
    .bind(req.file_id)
    .fetch_optional(&mut *conn)
    .await?;
    let Some((state_text, drive_id)) = row else {
        return Err(TransitionError::NotFound);
    };
    let Some(from) = FileState::parse(&state_text) else {
        return Err(TransitionError::UnknownState(state_text));
    };
    if req.expect_from.is_some_and(|expected| expected != from) {
        return Err(TransitionError::Conflict { actual: state_text });
    }
    if !is_legal(Some(from), req.to) {
        return Err(TransitionError::Illegal { from, to: req.to });
    }

    sqlx::query(
        "UPDATE _oo_docs.file SET state = $2, state_since = clock_timestamp(), state_code = $3, \
         retryable = $4, revision = revision + 1, \
         progress = CASE WHEN $2 IN ('parsing', 'extracting') THEN progress ELSE NULL END \
         WHERE id = $1::uuid",
    )
    .bind(req.file_id)
    .bind(req.to.as_str())
    .bind(req.code)
    .bind(req.retryable)
    .execute(&mut *conn)
    .await?;

    let history_seq: i64 = sqlx::query_scalar(
        "INSERT INTO _oo_docs.file_state_history \
         (file_id, seq, version_id, from_state, to_state, code, attempt, detail) \
         SELECT $1::uuid, COALESCE(max(seq), 0) + 1, $2::uuid, $3, $4, $5, $6, $7::jsonb \
         FROM _oo_docs.file_state_history WHERE file_id = $1::uuid RETURNING seq",
    )
    .bind(req.file_id)
    .bind(req.version_id)
    .bind(from.as_str())
    .bind(req.to.as_str())
    .bind(req.code)
    .bind(req.attempt)
    .bind(req.detail_json)
    .fetch_one(&mut *conn)
    .await?;

    let payload = format!(
        "{{\"file_id\":\"{}\",\"from\":\"{}\",\"to\":\"{}\",\"code\":{}}}",
        req.file_id,
        from.as_str(),
        req.to.as_str(),
        match req.code {
            Some(c) => format!("\"{}\"", c.replace(['\\', '"'], "")),
            None => "null".to_string(),
        }
    );
    let event_seq = events::append_event(
        conn,
        "file.state_changed",
        drive_id.as_deref(),
        Some(req.file_id),
        &payload,
    )
    .await?;
    Ok(Transitioned {
        from,
        to: req.to,
        history_seq,
        event_seq,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use FileState::*;

    #[test]
    fn spec_table_rows_are_legal() {
        let legal: &[(Option<FileState>, FileState)] = &[
            (None, Stored),
            (Some(Stored), TooLarge),
            (Some(Stored), Unsupported),
            (Some(Stored), Queued),
            (Some(Stored), Duplicate),
            (Some(Queued), Unsupported),
            (Some(Queued), Encrypted),
            (Some(Queued), Parsing),
            (Some(Parsing), Parsed),
            (Some(Parsing), Queued),
            (Some(Parsing), ParseFailed),
            (Some(Parsed), Extracting),
            (Some(Extracting), Indexed),
            (Some(Extracting), ExtractPartial),
            (Some(Indexed), Published),
            (Some(ExtractPartial), Published),
            (Some(Published), Outdated),
            (Some(Outdated), Parsing),
            (Some(ParseFailed), Queued),
            (Some(ExtractPartial), Queued),
            (Some(Published), Superseded),
            (Some(Published), Deleted),
            (Some(Superseded), Stored),
        ];
        for (from, to) in legal {
            assert!(is_legal(*from, *to), "{from:?} -> {to:?}");
        }
    }

    #[test]
    fn obvious_shortcuts_are_illegal() {
        let illegal: &[(Option<FileState>, FileState)] = &[
            (None, Queued),
            (Some(Stored), Parsed),
            (Some(Queued), Published),
            (Some(Parsed), Indexed),
            (Some(Parsing), Published),
            (Some(Published), Queued),
            (Some(Deleted), Queued),
            (Some(Deleted), Superseded),
            (Some(Superseded), Superseded),
            (Some(Encrypted), Queued),
            (Some(TooLarge), Queued),
        ];
        for (from, to) in illegal {
            assert!(!is_legal(*from, *to), "{from:?} -> {to:?}");
        }
    }

    #[test]
    fn names_round_trip() {
        for s in FileState::ALL {
            assert_eq!(FileState::parse(s.as_str()), Some(s));
        }
        assert_eq!(FileState::parse("weird"), None);
    }

    /// Deterministic xorshift, standing in for proptest (not yet admitted).
    struct Rng(u64);
    impl Rng {
        fn next(&mut self) -> u64 {
            self.0 ^= self.0 << 13;
            self.0 ^= self.0 >> 7;
            self.0 ^= self.0 << 17;
            self.0
        }
        fn pick(&mut self, n: usize) -> usize {
            (self.next() % n as u64) as usize
        }
    }

    /// Random walks only ever use legal edges, every state is reachable from
    /// creation, and `Deleted` is escapable only to `Stored`.
    #[test]
    fn random_walks_respect_the_table() {
        let mut rng = Rng(0x9e37_79b9_7f4a_7c15);
        let mut seen = std::collections::HashSet::new();
        for _ in 0..2_000 {
            let mut cur = Stored;
            seen.insert(cur);
            for _ in 0..60 {
                let moves: Vec<FileState> = FileState::ALL
                    .into_iter()
                    .filter(|t| is_legal(Some(cur), *t))
                    .collect();
                assert!(!moves.is_empty(), "{cur:?} is a dead end");
                let next = moves[rng.pick(moves.len())];
                assert!(is_legal(Some(cur), next));
                if cur == Deleted {
                    assert_eq!(next, Stored);
                }
                cur = next;
                seen.insert(cur);
            }
        }
        assert_eq!(seen.len(), FileState::ALL.len(), "unreachable states");
    }

    /// For random pairs, legality agrees with an independently written
    /// adjacency list.
    #[test]
    fn matches_adjacency_list() {
        let main: &[(FileState, &[FileState])] = &[
            (
                Stored,
                &[TooLarge, Unsupported, Queued, Duplicate, ParseFailed],
            ),
            (Queued, &[Unsupported, Encrypted, Parsing]),
            (Parsing, &[Parsed, Queued, ParseFailed]),
            (Parsed, &[Extracting]),
            (Extracting, &[Indexed, ExtractPartial]),
            (Indexed, &[Published, Outdated]),
            (ExtractPartial, &[Published, Queued, Outdated]),
            (Published, &[Outdated]),
            (ParseFailed, &[Queued, Outdated]),
            (Unsupported, &[Outdated]),
            (Duplicate, &[Outdated]),
            (Outdated, &[Parsing]),
            (Superseded, &[Stored, Deleted]),
            (Deleted, &[Stored]),
        ];
        let mut rng = Rng(42);
        for _ in 0..10_000 {
            let from = FileState::ALL[rng.pick(16)];
            let to = FileState::ALL[rng.pick(16)];
            let mut expect = main
                .iter()
                .find(|(f, _)| *f == from)
                .is_some_and(|(_, ts)| ts.contains(&to));
            if to == Superseded && !matches!(from, Superseded | Deleted) {
                expect = true;
            }
            if to == Deleted && from != Deleted {
                expect = true;
            }
            assert_eq!(is_legal(Some(from), to), expect, "{from:?} -> {to:?}");
        }
    }
}
