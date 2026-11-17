# FM2 review: file state machine (`formal/quint/file_lifecycle.qnt`)

Model of 00 section 5.1 / 10 section 4.2 against `crates/oo-docs/src/state.rs` (`EDGES`, `is_legal`,
`transition_in_tx`). Card V-S2-02 (md-subset model part). Status: author mapping and checks; the independent
80 section 5 step 2 review and the conformance side (Rust `conformance_state.rs`, card V-S3-01) are open.

## Mapping

| Spec 10 4.2 row | `state.rs` | Model action |
|---|---|---|
| (none) -> stored | `is_legal(None, Stored)` | `create` |
| stored -> too_large / unsupported; queued -> unsupported / encrypted; parsing -> parse_failed; extracting -> indexed / extract_partial | `EDGES` | `plain` |
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

`EDGES` in the model is a separate copy of the table, written from 10 4.2, so a cross-check against
`is_legal` for all 16 x 16 pairs is a meaningful follow-up (KP7 / V-S3-01); the table is exported as `EDGES` + `legal`.
One reading is inherited from `state.rs` and flagged: `Superseded -> Deleted` is legal, `Deleted -> Superseded` is not.

## Invariants (`inv`)

`invOnlyTableTransitions` (every step is in the table), `invPublishedFromLatest` (facts only from the
latest version of a live file), `invNoStaleOverwrite` (a state that holds a parse result has the current
version's result), `invDuplicateSameHash`, `invNoDeadEnd` (every state has a table successor; the no-permanent-
intermediate-state liveness is not model checked).

## Not covered

Rename (no state change), `blocked_reason`, `pending_review`, events, history rows, job lease
interaction (FM1), the sidecar and media paths, per-version history beyond one file row, and 3 versions x 3
files is the whole bound. `too_large`/`unsupported`/`encrypted` are reachable from any file regardless of
size or sniff result (the model does not carry the cause).
