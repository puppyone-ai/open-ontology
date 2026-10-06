# Findings

Verdict: **APPROVE_LIMITED**.

Reviewer is this session. Four passes, no subagents. Scope is `tools/session-release` at commit `524c6f5197a0e31e8d9e8f3e06510c7ee3d1d823` (crate tree `2a408d25eca5a076b1f6295b7ce40b8797f70526`) and this evidence directory. The fixed input archive is `c8588faab992af1193ff58dbc489a0b117796aa0`. The decisive run is `linux/impl-5`.

This review does not accept the parent task, does not close SP11, and does not certify a physical release. `physical_release_certified` stays false. The session stays `releasing`.

## Pass 1 — reducer contract

`drive.rs` sends `Event::Cancel` and `Event::RecordReleaseStep` with status `Confirmed`. It has no `CompleteRelease` constructor. `actual_absence` is sent only for S8. A reducer result that is `Closed` or `physical_release_certified` from either of those events is `unexpected_close` and is not treated as success.

Exit 0 requires S1, S2, S3, S4, and S8 confirmed both here and in the reducer, S8 evidence containing `local_fixture_directory` and `actual_absence` true, S5–S7 and S9–S11 still absent in the reducer, every unsupported action still `unsupported`, phase `releasing`, and `slo_miss` false. S5 is left unset. It is not skipped.

The live success test reached that exit. A following `BeginTurn` was `AdmissionStopped`. `get` showed phase `Releasing`, failure `UserCancel`, and `s11` empty. Revision conflict left phase `running` and the revision unchanged, with no `harness_armed` marker. Reusing a heartbeat event id as the cancel id returned `event_id_conflict` and left the phase unclosed.

## Pass 2 — clocks, harness, and paths

Cancel is persisted and the checkpoint wall epoch is fsynced before the harness is spawned. Fresh deadlines use the max of monotonic elapsed and wall elapsed. A restart uses only the persisted wall epoch. Exactly 5000, 10000, and 60000 ms are inside the budget. The clock unit test covers the next millisecond.

The harness is a direct child with its own process group. The signal is `/bin/bash` `kill -s` of the negative group id. The impl-5 signal probe exited 0 inside the cap-dropped container. S1 is confirmed only when the direct child has exited, `/proc` was readable, and every snapshotted descendant pid and starttime is gone.

The success fixture traps SIGTERM, sleeps 6 seconds, then exits. Its receipt had harness elapsed at least 5000 ms and backend elapsed strictly smaller, so the backend branch finished while the harness branch was still waiting. The idle-transaction fixture's harness elapsed was under 2000 ms while the backend branch stayed active until at least 4900 ms. Those two receipts are the evidence that the 5 second branch and the 10 second wait are independent. The harness timestamp is taken on the blocking thread when `stop` returns, not when the orchestrator later joins that thread.

The restart test SIGKILLed the orchestrator after `harness_armed`. The second process resumed, set `deadline_basis` to `wall_epoch`, reported S1 `harness_not_owned`, left the recorded child alive, left the workspace in place, and did not certify. A missing recorded pid would be `unproven_absence` and would not confirm S1.

Path checks run before any store call. A missing workspace returned `workspace_missing` with `database_used` false. A foreign directory kept `keep.txt`. A symlink inside an owned tree fails the unit destroy test and leaves the note file. A transcript longer than 65536 bytes left S4 unconfirmed and left the workspace in place. The noisy child's stdout is a file, and that receipt JSON stayed under 20000 bytes.

Residual: if `proc_snap` fails after `Command::spawn`, the `Child` is dropped without a signal. That path returns `proc` and does not confirm S1. The process can remain.

## Pass 3 — backend observation

`confirmable` is true only for `observed_idle` or `observed_absent` when `slo_miss` is false. `cancel_return_was_sufficient` is always false. `idle` is the only state treated as stopped. `idle in transaction` stays active. A closed socket, a missing pid, a foreign identity, and a still-active backend at the deadline are not confirmed.

The blocked `pg_sleep` backend was confirmed as idle or absent inside 5 seconds, and S2 was stored before the slow harness finished. The open `BEGIN; SELECT 1` backend stayed `idle in transaction`. The cancel role observed that state after the deadline. `pg_cancel_backend`'s boolean was recorded and was not success. The same test showed backend elapsed at least 4900 ms and below 8000 ms.

The cancel login is `oo_session_cancel` and the runtime login is `oo_session_runtime`. The live privilege test saw `rolsuper` false for the cancel role, SQLSTATE `42501` on `UPDATE oo_session_control.session_head`, and `42501` on a runtime `CREATE TABLE`. The orchestrator issues no grants. Port 1 returned exit 3, `pg_unavailable`, `database_used` false, no harness marker, and phase `Running`.

Residual: a dropped cancel-role socket is `socket_closed` and the unit test rejects it. No live test cuts that socket in the middle of the poll.

## Pass 4 — tests and evidence

`linux/impl-5` is the decisive run. `docker-run.exit`, `test.exit`, and `clippy.exit` are 0. Lib tests: 9 passed. Live tests: 9 passed in 15.00 s. Lock subset: 1 passed. Clippy was `cargo clippy --locked --offline --all-targets -- -D warnings`. The container had 6442450944 bytes of memory, 1000000000 nano CPUs, `CapDrop ["ALL"]`, `Privileged` false, and `OOMKilled` false. Network mode was `oo-a295-eef831aa-net`. The signal probe exited 0. Rustc was 1.94.0, host `aarch64-unknown-linux-gnu`.

The lock-subset test compared registry checksums with `tools/session-store/Cargo.lock` and with the archived root `Cargo.lock` from `c8588faab992af1193ff58dbc489a0b117796aa0`. No `cargo update` and no `cargo generate-lockfile` ran. a207 remains the root-lock writer.

The sixteen files hashed in `linux/impl-5/source-sha256.txt` match the host tree that was committed. Earlier attempts are preserved and are not reread as the product result:

| Run | docker-run.exit | What the log shows |
|---|---|---|
| `linux/precode` | 0 | Fixed transition 12 tests, store lib 11 tests, session-release absent |
| `linux/impl` | 101 | `E0614` on `*step` after `into_iter`; `unused_mut` |
| `linux/impl-2` | 101 | `event_prefix` was `&str`; dead `Armed` paths; `collapsible_if` |
| `linux/impl-3` | 101 | Live suite except the idle-transaction timing assertion (`harness 5008`, `backend 5007`); Clippy `zombie_processes` |
| `linux/impl-4` | 101 | Clippy 0. Live bootstrap returned `BootstrapConflict` because session ids were reused in the same database |
| `linux/impl-5` | 0 | Decisive tests and Clippy |

`pg/up.exit` is 0. The database container published no host port.

Residual: the 60 second resource deadline is the elapsed comparison plus the clock unit test. No live test waits past 60 seconds before destroy. Identity mismatch and a backward wall clock fail closed in code and have no separate live test.

## What this approval covers

The local-fixture orchestrator at `524c6f5197a0e31e8d9e8f3e06510c7ee3d1d823`, as exercised by `linux/impl-5`. Supported work is the direct-child harness, the limited backend cancel and observation, bounded collection, and UUID-owned directory destroy. Authentication, token revoke, proxy mapping, permits, network, runsc, sandbox, snapshot encryption, and the 30 second reaper stay unsupported. Containment is `local_fixture`.
