# Findings

Verdict: **APPROVE_LIMITED**.

The decisive execution is `linux/preoverlay` on archive `72dc9e2db841de4de368216bbbe13d738f850ea9`. The author's four passes are not this approval. `physical_release_certified` stays false. The session stays `releasing`.

## What the live tests executed

The nine live tests are the author's fixture, run here against a new TLS server and new role passwords. They passed in 15.16s with `--test-threads=1`. The assertions below are the ones in `tools/session-release/tests/live.rs` at that commit. A failing bound would have panicked with the measured number. This stdout does not print the numbers on success.

`blocked_query_and_slow_child_release_without_closing` persists cancel, then runs a direct child that sleeps 6 seconds after SIGTERM while a `pg_sleep` backend is cancelled on the `oo_session_cancel` connection. It requires exit 0, disposition `released_unclosed`, reason `local_fixture_not_certified`, phase `releasing`, `complete_release_called` false, and S5–S7 and S9–S11 `unsupported` in the orchestrator and `absent` in the reducer. S1, S2, S3, S4, and S8 must be `confirmed` on both sides. S8 evidence must contain `local_fixture_directory` and `actual_absence` true. S3 and S4 evidence must contain the SHA-256 of the bounded bytes. Backend elapsed must be at most 5000 ms, harness elapsed at least 5000 ms, and the backend elapsed strictly smaller. The workspace must then be `NotFound`. A later `BeginTurn` must be `AdmissionStopped` while `get` still shows `Releasing` and empty S11. The receipt must not contain the fixture passwords.

`cancel_cannot_treat_an_idle_transaction_as_stopped` leaves `BEGIN; SELECT 1` open. It requires exit 2, `slo_miss`, basis `still_active`, S2 orchestrator `still_active` and reducer `absent`, backend elapsed at least 4900 ms and below 8000 ms, and harness elapsed under 2000 ms. The S2 preimage must contain `cancel_return_was_sufficient` false. After the run, the cancel role must still read `idle in transaction`. That is the live rejection of a boolean cancel as success. It is not a closed socket.

`restart_after_the_orchestrator_is_killed_does_not_signal_or_certify` waits for `harness_armed`, SIGKILLs that orchestrator, and starts a second process on the same request. The second receipt must be exit 2, `resumed` true, `deadline_basis` `wall_epoch`, S1 `harness_not_owned`, not `released_unclosed`, phase `releasing`. The workspace must remain, and `/proc/<recorded pid>/stat` must still contain the recorded starttime. The test then kills that child itself. The orchestrator did not.

`revision_conflict_and_event_id_conflict_do_not_close` requires a stale revision to return `revision_conflict` with phase `running` and the stored revision unchanged, and no `harness_armed` marker. Reusing the heartbeat event id as the cancel id must return `event_id_conflict` and leave the phase unclosed, with S1 empty.

`noisy_transcript_is_not_confirmed_and_workspace_remains` writes 65537 bytes and requires S4 `too_big`, S8 unconfirmed, the workspace still present, and a receipt under 20000 bytes. `missing_workspace_and_foreign_path_have_no_side_effects` requires `workspace_missing` with `database_used` false, and `workspace_foreign` while `keep.txt` is unchanged and the phase stays `Running`. `inaccessible_postgres_does_not_spawn_or_close` uses port 1 and requires exit 3, `pg_unavailable`, `database_used` false, no harness marker, and phase `Running`. `cancel_role_is_limited_and_runtime_cannot_create` requires `rolsuper` false and SQLSTATE `42501` for a cancel-role `UPDATE` of `session_head` and a runtime `CREATE TABLE`.

The library test `cancel_return_and_socket_drop_are_not_success` requires `confirmable` to be false for `socket_closed`, `still_active`, `backend_unknown`, `stopped_after_deadline`, and a missed `observed_idle`. The clock test requires 5000, 10000, and 60000 ms inside the budget and the next millisecond outside. No live test waits past 60 seconds, and no live test cuts the cancel socket in the middle of the poll. Those two limits stay as they were in the author note. They were not widened into a new campaign.

`escaped_grandchild_and_canary_are_not_confirmed` also passed. It is one of the nineteen. It was not added by this review.

## proc_snap after spawn

`harness::spawn` spawns the child and then calls `proc_snap`. On `None`, or when the snap pid differs, the function returns `proc` and drops `Child`. Drop does not signal or wait. `fresh` treats that error like any other spawn error: it sets the receipt reason and returns through `conclude` without confirming S1 and without `actual_absence`. `success_ready` cannot turn that into exit 0. The reason is `proc`, not `absent` and not `unproven_absence`.

This container can read `/proc`, and `spawn` is private to the crate, so this review did not induce the failure. The residual is the dropped child. It is not success and it is not a false absence. No product change and no extra child were added to force it.

## Locks and scope

Every session-release registry triple matches session-store. All 128 are in the archived root lock. All 11 reducer registry triples are in both the store lock and the root lock. Path packages `session-release`, `session-store`, and `session-transition` are not registry packages. The root lock was not edited. a207 remains the only writer of the connector manifest and the root lock.

The supported adapters exercised here are the direct-child harness, the limited backend cancel and observation, bounded collection, and UUID-owned directory destroy. Authentication, token revoke, proxy mapping, permits, network, runsc, sandbox, snapshot encryption, and the 30 second reaper stay unsupported. The binary does not issue grants. The fixture installer, which is not the orchestrator, creates the roles.

Passwords were files mounted into the container. They were not process arguments or environment variables of `docker run`. The evidence directory does not contain them.
