# session-transition v1

Pure function from one caller-supplied snapshot to the next snapshot.
Precursor for a later durable compare-and-swap adapter.
This crate is not a database, an authenticator, a sandbox driver, or a clock.

`physical_release_certified` is always false.
`database_used`, `auth_provider_used`, and `runtime_used` are always false.
Recording an S8 `actual_absence` flag stores the caller's assertion.
The reducer does not inspect a host, a container, a volume, or a tmpfs.

Pinned dependencies, taken from the repository root lock and not edited here:

| crate | version | checksum |
|---|---|---|
| serde | 1.0.229 | `4148590afebada386688f18773da617792bf2ef03ffc1e4cbd2b1d45b023e0ba` |
| serde_core | 1.0.229 | `67dca2c9c51e58a4791a4b1ed58308b39c64224d349a935ab5039aa360942a48` |
| serde_derive | 1.0.229 | `e7a5d71263a5a7d47b41f6b3f06ba276f10cc18b0931f1799f710578e2309348` |
| serde_json | 1.0.151 | `c841b55ecdae098c80dcae9cf767f6f8a0c2cdb3416bbef72181df4d0fe73f14` |

No path dependency. No other direct dependency. `rust-version` is 1.94.0. Edition 2024.
Safe Rust only.

## Clock and budgets

`now_ms` is an explicit input. The crate does not read the system clock.

| bound | milliseconds or count | admission rule |
|---|---|---|
| idle | 1_800_000 | `begin_turn` requires `now_ms - last_activity_at_ms < 1_800_000` |
| absolute | 28_800_000 | admission requires `now_ms - session_started_at_ms < 28_800_000` |
| turn | 600_000 | `tool_start`, `tool_finish`, and `finish_turn` require `now_ms - turn.started_at_ms < 600_000` |
| tool starts | 60 | `tool_start` requires `tool_starts < 60` |
| owner lease | 30_000 | a new expiry must satisfy `0 < expiry - now_ms <= 30_000` |

The boundary instant is not admissible. `elapsed == bound` refuses.
A later explicit timeout event is what moves the phase. An admission refusal does not.

`now_ms < last_now_ms` is `time_backward` for every event, including a replay.
`now_ms == last_now_ms` is allowed.

`last_activity_at_ms` moves on `begin_bind`, `complete_bind`, `begin_turn`, `finish_turn`, `tool_start`, `tool_finish`, and `turn_timeout`.
Heartbeat updates `lease_expires_at_ms` and `last_now_ms` only.
It does not move `last_activity_at_ms`.
Cancel, takeover, idle timeout, absolute timeout, and release inputs do not move `last_activity_at_ms`.

## Identity

The identity tuple is caller-verified before the call.
This crate does not check signatures, OIDC, SCIM, or `roles.yaml`.
Self-asserted claims inside an event are not a field. Unknown JSON fields are rejected.

The tuple is `project_id`, `principal_id`, `role`, `claims_hash`, `policy_version`, `knowledge_version`, `config_version`.
`claims_hash` is 64 lowercase hex digits. The other fields are tokens.

`begin_bind` from `requested` stores that tuple.
After `bound` is present, every new event id whose tuple differs in any field returns `new_session_required`.
The state is unchanged, including `workspace_id`.
`same_workspace_allowed` is false. The caller opens a different session.
This crate does not delete or reassign the recorded workspace.

Tokens are 1 to 128 bytes, ASCII alphanumeric plus `.` `_` `:` `/` `-`.

## Phases

`requested → binding → ready → running ⇄ idle → releasing → closed`

Failure is not a phase. `failure` is a separate field.

| event | from | to |
|---|---|---|
| `begin_bind` | requested | binding |
| `complete_bind` | binding | ready |
| `begin_turn` | ready, idle | running |
| `finish_turn` | running | idle |
| `tool_start` | running | running |
| `tool_finish` | running | running |
| `heartbeat` | binding, ready, running, idle, releasing | same |
| `takeover` | binding, ready, running, idle | releasing |
| `takeover` | releasing, generation ≥ 1 | releasing |
| `cancel` | requested, binding, ready, running, idle | releasing |
| `turn_timeout` | running | idle |
| `idle_timeout` | ready, idle | releasing |
| `absolute_timeout` | binding, ready, running, idle | releasing |
| `record_release_step` | releasing | releasing |
| `complete_release` | releasing | closed |

`ready` means bound to a workspace and no turn is active yet.
`idle` means a turn finished or timed out and the session remains.
`running` means one turn is active.
`releasing` admits no turn and no tool call.
`closed` admits nothing except an identical journal replay.

There is one role string, not a list.
`begin_bind` requires `owner_generation == 0` and sets it to 1.
The absolute and activity clocks start at `begin_bind`'s `now_ms`.

## CAS

Inputs include `expected_revision` and `expected_owner_generation`.
For a new event id, both must equal the supplied state's fields.
A mismatch is `stale`. Revision, generation, journal, and every other field stay as supplied.
This function does not write to a database.
A later adapter must persist the output only with a storage CAS on those same numbers.

`takeover` and `begin_bind` are the only generation changes.
`takeover` uses `checked_add(1)`. `u64::MAX` is `generation_overflow` and stays `u64::MAX`.
A successful apply uses `checked_add(1)` on revision.
`u64::MAX` is `revision_overflow` and stays `u64::MAX`.
Lease maths use checked subtraction. A delta that does not fit the rule is `lease_not_future` or `lease_too_long`.
No wrapping add is used for revision, generation, or lease.

Phase and boundary errors are reported when the expected numbers match.
Overflow is reported only when the event would otherwise commit.
A full journal is `dedup_exhausted` only at that same commit point.

## Journal

Cap is 1024 entries. The cap is inside the state the caller supplies.
A new event id at cap refuses `dedup_exhausted`.
No entry is dropped. Length and order stay the same.
An id already in the journal still resolves when the journal is full.

The fingerprint is SHA-256 of the compact JSON object, field order fixed:

1. `event_id`
2. `identity` (project, principal, role, claims hash, policy, knowledge, config)
3. `event`

`now_ms`, the expected numbers, and the state are not part of the fingerprint.
SHA-256 is the FIPS 180-4 function in this crate.
Empty input is `e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855`.
`abc` is `ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad`.

Same id and same fingerprint: outcome `idempotent`, state unchanged.
This holds even when `expected_revision` is the pre-image value.
The output revision is the supplied state's revision.
Same id and a different fingerprint: `event_id_conflict`, state unchanged.
Refusals are not journaled. A corrected new attempt may reuse that id.
A journal longer than 1024, a duplicate id, or a bad hash is `state_invalid`.

1024 events do not cover an 8 hour trace of 30 second heartbeats plus the maximum tool traffic.
Exhaustion fails closed, including for a later release event that was not already journaled.
That limit is this precursor's bound, not a silent eviction.

## Lease

`heartbeat` requires `owner_id` equal to the state's owner and `now_ms < lease_expires_at_ms`.
Otherwise `owner_mismatch` or `lease_expired`.
At the exact expiry instant, heartbeat refuses and takeover may proceed.

`takeover` requires `now_ms >= lease_expires_at_ms`, a new token owner id, and a future lease within 30_000 ms.
Early takeover is `lease_held`.
From a live phase it sets `failure` to `owner_lost`, clears the turn and tool tables, and enters `releasing`.
From `releasing` it keeps the existing failure and only replaces owner, generation, and lease.

Admission (`complete_bind`, `begin_turn`, `finish_turn`, `tool_start`, `tool_finish`) requires a live lease: `now_ms < lease_expires_at_ms`.
Cancel, the three timeout events, takeover, and release inputs do not require a live lease.
They still require a matching generation when they are new ids.

## Turns and tools

`begin_turn` is refused with `lease_expired`, `absolute_boundary`, or `idle_boundary` before the phase changes.
The checks run in that order.
A new turn sets `tool_starts` to 0 and clears both tool tables.

`tool_start` checks lease, absolute, turn boundary, then `tool_starts < 60`, then a new tool id.
The 60th successful start leaves `tool_starts == 60`.
The next start is `tool_boundary` and the count stays 60.
Reusing a tool id in the same turn is `tool_id_reused`.
`tool_finish` removes an open id and does not change `tool_starts`.
An id that is not open is `tool_not_open`.

`finish_turn` requires the matching turn id, no open tools, a live lease, and the turn boundary.
It enters `idle` with `last_turn_outcome = finished` and `tool_starts = 0`.
`turn_timeout` requires the matching turn id and `now_ms - started_at_ms >= 600_000`.
It clears open and started tools, enters `idle`, and sets `last_turn_outcome = timed_out`.
It does not enter `releasing`.
Before the boundary the reason is `turn_not_reached`.

`idle_timeout` from `ready` or `idle` requires the idle boundary and enters `releasing` with `failure = idle_timeout`.
Before the boundary the reason is `idle_not_reached`.
`absolute_timeout` from `binding`, `ready`, `running`, or `idle` requires the absolute boundary.
It clears a live turn and enters `releasing` with `failure = absolute_timeout`.
Before the boundary the reason is `absolute_not_reached`.
From `requested` the reason is `not_started`.

## Cancel and release

`cancel` triggers are `user`, `admin`, and `entry_lost`.
They map to `user_cancel`, `admin_terminate`, and `entry_lost`.
From `requested` the session enters `releasing` with no bound tuple and no workspace.
From a later live phase it clears the turn and tool tables.
`already_releasing` and `closed` refuse a second cancel.

`releasing` refuses `begin_turn`, `finish_turn`, `tool_start`, `tool_finish`, `complete_bind`, and `begin_bind` with `admission_stopped`.

`closed` is reachable only by `complete_release`.
That event carries all eleven steps, S1 through S11.
Each step has `status`, `evidence_hash` (64 lowercase hex), and `actual_absence`.
S5 may be `confirmed` or `skipped`.
Every other step must be `confirmed`.
`actual_absence: true` is valid only on confirmed S8.
Any other true flag is `absence_only_s8`.
S8 with `actual_absence: false` does not close: `s8_not_absent`.
A missing step is `release_incomplete`.
A bad status is `step_status`.

If a step was already recorded and the completion payload differs, `release_mismatch`.
`record_release_step` may replace a previous step with a new event id.
Replacement is an explicit input and bumps revision when it commits.
`complete_release` writes any still-empty steps from its payload, then sets `closed`.
The output flag `physical_release_certified` remains false.

## Decision order

1. `state_invalid` or `input_invalid`
2. `time_backward`
3. Journal hit: `idempotent` or `event_id_conflict`
4. New id, bound tuple differs: `new_session_required`
5. New id, expected revision or generation differs: `stale`
6. Event rule refusal, with no write
7. Journal at 1024: `dedup_exhausted`
8. Revision or generation increment would overflow: `revision_overflow` or `generation_overflow`
9. Commit, append the journal entry, set `last_now_ms`, add 1 to revision

## JSON

`TransitionInput.contract` and `TransitionOutput.contract` are `session-transition-v1`.
Any other input contract string is `input_invalid`.
Unknown fields are rejected.
Phases, reasons, outcomes, failures, and step ids are snake case.
`owner_id` is empty only when `owner_generation` is 0.
Tool id vectors are strictly sorted.
`release` always has keys `s1` through `s11`, each a step record or null.

## CLI

The `session-transition` binary takes no arguments and no environment identity.
It reads one JSON `TransitionInput` from stdin, at most 1_048_576 bytes.
A larger stdin is refused without further reads.
Trailing bytes after the JSON value are rejected.
Stdout is one JSON `TransitionOutput` for a parsed command.
Exit 0 is `applied` or `idempotent`.
Exit 2 is `refused`.
Exit 1 is malformed stdin, usage, or an I/O error.
Stderr on a refusal is one line: `session-transition: refused <reason>`.
The stdout `revision` equals `state.revision` and is the post-state value on success.
On refusal it is the supplied revision, unmodified.
Malformed stdout has no revision, because no state was accepted.

## Outside this crate

Durable row CAS, signature checks, OIDC, pool allocation across sessions, gVisor, token revoke, and physical S1–S11 actions stay outside.
The caller that wants a new actor uses a new session id and a new workspace.
This reducer will not attach the old workspace to the new tuple.
