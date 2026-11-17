# FM2 review: file state machine (`formal/quint/file_lifecycle.qnt`)

Model of 00 section 5.1 / 10 section 4.2 against `crates/oo-docs/src/state.rs` (`EDGES`, `is_legal`,
`transition_in_tx`). Cards V-S2-02 (md subset) and V-S3-01 (full machine: all 16 states, quarantine, manual retry,
KP7, `conformance_state.rs`). Status: author mapping and checks; the independent 80 section 5 step 2 review is open.

## Mapping

| Spec 10 4.2 row | `state.rs` | Model action |
|---|---|---|
| (none) -> stored | `is_legal(None, Stored)` | `create` |
| stored -> parse_failed (object unreadable, 10 4.3.2; not a row of the 4.2 table) | `EDGES` | `failParse` with code `unreadable` |
| parsing -> parse_failed (attempts used up, not retryable, or the blob was quarantined, 10 4.1.6) | `EDGES` | `failParse` (`exhausted`, `quarantined`; a quarantined content hash goes on `poison`) |
| parse_failed -> queued (manual retry, also of a quarantined file) | `EDGES` | `toQueued` |
| stored -> too_large / unsupported; queued -> unsupported / encrypted; extracting -> indexed / extract_partial | `EDGES` | `plain` |
| stored -> queued (blob new or without a successful result) | `EDGES` | `toQueued` (guard: no completed blob with the same hash) |
| stored -> duplicate (blob already has parse + extract) | `EDGES` | `toDuplicate` (guard: another file with the same hash is `indexed` or `published`) |
| queued / outdated -> parsing | `EDGES` | `toParsing` (records `jobVer`, the version the job works on) |
| parsing -> parsed | `EDGES` | `finishParse` (only if `jobVer == ver`; a late result of an older version is dropped) |
| parsing / extract_partial / parse_failed -> queued | `EDGES` | `toQueued` |
| indexed / extract_partial -> published | `EDGES` | `toPublished` (needs `resVer == ver`) |
| published etc. -> outdated | `EDGES` | `toOutdated` (published facts stay, D23) |
| any active -> superseded (new blob) | `to == Superseded` rule | `supersede` (version + 1, facts withdrawn, job may still run) |
| any -> deleted | `to == Deleted` rule | `remove` (facts withdrawn) |
| superseded / deleted -> stored | `EDGES` | `restore` |

`EDGES` in the model is a separate copy of the table, written from 10 4.2 plus the 4.3.2 row. Its pairs are read
from the `.qnt` text by `conformance_state.rs::fm2_transition_table_equals_is_legal` and compared with
`is_legal` for all 16 x 17 pairs (no database needed); `state/proofs.rs` (KP7) proves `is_legal` equal to a third
statement of the table for the same pairs. One reading is inherited from `state.rs` and flagged:
`Superseded -> Deleted` is legal, `Deleted -> Superseded` is not. Spec finding: the 10 4.2 table has no
`stored -> parse_failed` row although 4.3.2 and `state.rs` have it (the 4.2 table should gain it).

## Invariants (`inv`)

`invOnlyTableTransitions` (every step is in the table), `invPublishedFromLatest` (facts only from the
latest version of a live file), `invNoStaleOverwrite` (a state that holds a parse result has the current
version's result), `invDuplicateSameHash`, `invNoDeadEnd` (every state has a table successor; the no-permanent-
intermediate-state liveness is not model checked), `invQuarantineShape` (`parse_failed` carries a reason, no other
state does) and `invQuarantinePoison` (a quarantined file's content is on the poison list). All 16 states are
witnessed by `quint run` (`witState_*`).

## Not covered

Rename (no state change), `blocked_reason`, `pending_review`, events, history rows, job lease
interaction (FM1), the pump's rule that a poison blob fails every path carrying it (the model lets any file
with a poison hash be retried), the sidecar and media paths, per-version history beyond one file row, and 3 versions x 3
files is the whole bound. `too_large`/`unsupported`/`encrypted` are reachable from any file regardless of
size or sniff result (the model does not carry the cause).
