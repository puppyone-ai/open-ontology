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

## CI bounds (card V-S1-01, time budget of 80 section 6)

Per invariant on a loaded 16 core machine (see FM1.md). The md-subset model at 10 steps: invNoDeadEnd 74 s,
invOnlyTableTransitions 82 s, invPublishedFromLatest 94 s, invDuplicateSameHash 215 s, invNoStaleOverwrite 1087 s (over
12 minutes). The full model (this card) at 8 steps: invPublishedFromLatest 46 s, invNoDeadEnd 48 s,
invOnlyTableTransitions 51 s, invDuplicateSameHash 111 s, invQuarantinePoison 142 s, invQuarantineShape 206 s,
invNoStaleOverwrite 403 s. FAST bound `verify-steps=8`; nightly `deep-steps=10` (the full model at 10 steps is not
measured).

## Independent review (native sub-agent, 2026-11-17, commit e82490f57)

Verdict: APPROVE WITH CHANGES. Reviewed: `file_lifecycle.qnt`, `state.rs` (`EDGES`, `is_legal`), `conformance_state.rs`, spec 00 section 5.1, 10 section 4.1.1 and 4.2. Checked: `quint typecheck` on all 9 models passes; `quint run --max-samples=2000 --seed=7` on `inv` finds no violation (no bound changed).

Blocking
1. **B1, the stale-result invariant does not see the real stale-overwrite case.** `finishParse` guards on `jobVer == ver` only inside `parsing`. But `ver` changes only through `supersede`, which also leaves `parsing`, so inside `parsing` `jobVer == ver` always holds. Mutant check: dropping the guard (`file_lifecycle.qnt` finishParse, first branch) and also stamping `resVer: fl.jobVer` both still give "No violation" for `inv`. The dangerous path is: job for v1 running, file superseded to v2, restored, leased again (`toParsing` overwrites the single `jobVer` slot with v2), then the v1 job completes and is accepted as current. The model cannot express that because it holds one job slot per file. Spec 10 4.1.1 closes this with the `(owner, generation)` CAS (`lost_lease`), which is FM1 (`jobs.qnt`) territory, not FM2. Required change: either (a) add a `leaseGen` counter to `File` that `toParsing` bumps and `finishParse` compares, with an invariant that a parsed result carries the generation of the lease that produced it, or (b) state in the FM2 header that the stale-after-relaunch case is discharged by FM1 and add that cross-reference to the invariant list. Option (a) is preferred.
2. **B2, the conformance test does not compare the version state.** `replay` compares only `st` and the `parse_failed` code. It passes `version_id: None` and `attempt: None` to `state::transition`, and it skips steps where state does not change (`finishParse` discards, `jobVer` changes). So `ver`, `hash`, `jobVer`, `resVer`, `pubVer`, `dupHash`, `poison` and `discards` are never compared with the database, and the version fence in `transition` is never exercised. Required: compare `ver`/`resVer`/`pubVer`/`dupHash` from the DB, and replay the discard steps (they can be detected from `discards` changing or from a new `jobVer` field in the trace).

Non-blocking
3. The guards of `toDuplicate` (another file with the same hash is `indexed` or `published`) and `toQueued` from `stored` (no completed blob) are model preconditions only. `is_legal` does not encode them and the conformance replay does not check them, so code that moves a file to `duplicate` without a completed blob would still pass. Suggest a note in the conformance doc, or a check in the replay that the blob precondition holds in the model state before the step.
4. `restore` from `deleted` requires `ver + 1 <= V`. At the bound a deleted file can never come back in the model. Say so in "Not covered" (the bound is V = 3).
5. `invNoStaleOverwrite` is only meaningful together with item 1; as written it holds by construction for every mutant I tried. Name it in the review as weak until item 1 is fixed.
6. The model's `EDGES` copy is re-read by the test from the `.qnt` text; that is good. Keep the 16 x 17 loop, but it currently checks `state.rs` only, not the `stored -> parse_failed` row in the spec (already noted by the author).

Checked and fine: `legal()` in the model and `is_legal` agree on `absent`, `superseded` and `deleted`; `supersede` from `absent` is blocked by `legal`; `plain` cannot reach `stored`, `queued`, `parsing`, `published`, `outdated`, `superseded`, `deleted`; the `unreadable` code only from `stored`.

## Response to the independent review (lane A, 2026-11-17)

- **B1:** reviewer option (a). `File` has a lease generation `gen`, the accepted result's `accGen`, and `jobs`, the parse jobs in flight as `(ver, gen)`
  pairs (a worker that lost its lease keeps running). `toParsing` raises `gen` and adds a job; `finishParse` picks one in-flight job and accepts it only when
  the file is `parsing` and the job's `(ver, gen)` is the file's (the CAS of 10 4.1.1), else discards it. Invariant `invResultFromCurrentLease`: a `parsed`
  file's result has the file's generation and version. Mutation M1 (accept on the version alone, a stale lease of the same version) and M3 (accept any
  job while parsing) are caught. M2 (accept on the generation alone) is **equivalent**, not a gap: a lease's generation already fixes its version, because a
  supersede always leaves `parsing` and the next lease takes a new generation.
- **B2:** the replay keeps one `file_version` row per model version (`ver` is the index of the current one) and checks the model's `resVer <= ver` and
  `pubVer in {-1, ver}`; for every `finishParse` step it runs the stage fence (`jobs/stages.rs::load_subject`: the job's version is still the file's current,
  not superseded, not deleted version) and asserts it equals the model's `j.ver == ver`. **Finding:** `state::transition` itself has no version fence
  (`version_id` is only logged); the fence is the stage's SQL, which the replay now exercises (copied query, the function is private). The generation half
  is FM1's `(owner, generation)` CAS, not replayed here. 300 traces, 8,344 steps, 84,036 probes pass.
- Not done: 3 (guards of `toDuplicate` / `toQueued` as replay preconditions), 6. 4: at the bound `V = 3` a deleted file whose `ver + 1 > V` cannot be
  restored in the model. 5: `invNoStaleOverwrite` stays weak on its own; `invResultFromCurrentLease` is the strong one.
