# Findings — a189 executor shutdown and role bind

Source `15d23fe9f2cbc74e3f3278d70d0f56390568126e`, tree `feec821ab54f0aaf0001218f90150befce2803b2`. Baseline `8a289f0c699abb8c707ca45ad45cb036abc3fc05`. a184 review evidence `df6067e9c6098f874b857d53fec139b78d6107c8`.

This repair changes the existing trusted entry. It does not close SP06-W04, SP06-W05, or SP06-W06, and it does not claim a release. The measure is `contracts/gateway/query-executor-v1.md` plus the private PostgreSQL 17.10 oracle in `live/oracle.txt`.

## F1. What the entry does now

`DedicatedSession::execute_trusted_select` (`crates/oo-gateway/src/query_executor.rs` line 606) is still the only entry. `UNTRUSTED_ADMISSION` is `Blocked` (line 103). The whole-query `Instant` is taken at entry (line 614). `statement_timeout` is still set once, to the remaining budget minus about 250 ms (lines 1514–1515). `CLEANUP_BOUND` is 750 ms (line 93). That ceiling covers a dropped call's server response and the following `ROLLBACK`. It does not refresh the `Instant` or `statement_timeout`.

After the session-open check and the row, byte, and wall limits, `role_bind_decision` (line 1111) runs. The only store of `bound_role` is line 637, and it runs only when the decision is `Proceed` and the session has no stored role. `guard_select` and `admit` run after that store. `run_producer` returns the client on success and drops it otherwise. It no longer stores the role.

Hot database calls go through `await_hot` (line 1183). The future is pinned and kept polled. If shutdown is already set, the call returns `Hot::Consumer` and is not started. If shutdown arrives while the future is polled, `send_bound_cancel` (line 1241) checks the stored generation, acceptance flag, pid, and `backend_start`, drops that lock, and calls `cancel_query`. A mismatch returns without calling `cancel_query`. There is no await between the check and the call, and this path does not open a second session. After the cancel is sent, the same future is polled until it is ready or 750 ms passes. A consumed response is classified from shutdown even when the server error is `57014` (`hot_result`, lines 1230–1233): `partial`, stop `Protocol`, step `consumer`, SQLSTATE absent. `rollback_bounded` (line 1639) then waits up to 750 ms. If the ceiling expires first, the status stays `partial` and rollback is not claimed.

A ready server error is preferred over the client sleep (`select` bias at lines 1204–1211). A client sleep that wins returns `Hot::Deadline`, status `failed`, stop `Timeout`, SQLSTATE absent, and drops the unread future. `CancelHandle::cancel` still observes `pg_stat_activity` from another session. A user-request `57014` that arrives without consumer shutdown stays `failed` / `Canceled` and keeps the SQLSTATE. Any other server `57014` stays `failed` / `Timeout` and keeps the SQLSTATE. A `complete` hook failure does not rewrite the status.

Stream drop sets the watch flag. It does not abort the producer task. `driver.abort` remains on a failed `bind` (line 578) and on `close` (line 727), after the client slot is cleared. The producer is joined from `wait_idle`.

## F2. Baseline, measured before the edit

`linux/probe/oracle.txt`, executor SHA-256 `ed3e1c4331d58fe79c449cdb06c6c39a40992bf20e3273d244566761c0605a1b`, PostgreSQL 17.10, TLS on.

A guard refusal left `bound_role` empty and the session open. The other valid role then succeeded (`other_role=allowed`, `other_role_bound=oo_r_west`). That is the a184 contract gap: the second role was refused only after a successful producer had stored the role.

A portal blocked in `pg_sleep` returned on drop in 4 ms as `partial` with rollback `no`. The select observed shutdown and abandoned the in-flight read. A portal blocked on `ACCESS EXCLUSIVE` returned in 5703 ms as `failed`, SQLSTATE `57014`, stop `timeout`, rollback `yes`. The drop waited for the server timer. a184 had measured the same shape at 2713 ms on a 3000 ms wall. This reproduction used a longer server limit and recorded 5703 ms.

`linux/probe/attempt-1/` panicked before the lock measurement (`locked query never became active`, exit 101). It is preserved. The deciding probe is `linux/probe/`.

## F3. Blocked drop on the repaired executor

`linux/proof/live.log`: 1 passed, 0 failed, finished in 15.51 s. `live/oracle.txt`.

`prove_held_drop` waits until the user query is active, sleeps 3100 ms, and asserts the backend is still active. The wall is 8000 ms, so `statement_timeout` is about 7750 ms. The server limit did not fire during the hold. Drop plus `wait_idle` is then required to finish under 1000 ms, with `partial`, stop `Protocol`, SQLSTATE absent, rollback confirmed, socket dropped, end audit ok, session closed, and the backend gone.

Measured:

| Case | Held active | Drop | Status | SQLSTATE | Stop | Rollback |
|---|---:|---:|---|---|---|---|
| `ACCESS EXCLUSIVE` | 3100 ms | 3 ms | `partial` | absent | `protocol` | yes |
| `fixture.page_hold()` (`pg_sleep`) | 3100 ms | 6 ms | `partial` | absent | `protocol` | yes |

Both are under the 750 ms cleanup ceiling and under the 1 s bound asked for this fixture. The baseline lock drop was 5703 ms and `failed`. The baseline sleep drop was 4 ms and `partial` with rollback `no`.

`prove_finished_drop_spares_next` ran a successful query, dropped that stream through the success path, then started `page_hold` on the same session. The new backend stayed active after 400 ms. Its own drop was `partial` at 6 ms (`finished_drop_next=still_active drop_ms:6`). The finished query's cancel identity was not applied to the next backend.

`prove_client_budget_spares_next` used wall 2000 ms on `page_hold`. The result was `failed`, stop `Timeout`, SQLSTATE `57014`, elapsed 1798 ms. The server timer, set to the remaining budget minus about 250 ms, answered before the client sleep. The status was not `partial`. The handle from that query was `NotSent` against a new session. That new `page_hold` stayed active after the stale cancel and after another 400 ms.

`prove_server_deadline` is the same classification on a lock: `failed` / `Timeout` / `57014` at 1796 ms. `prove_inflight_cancel` observed `CancelOutcome::Sent` and `failed` / `Canceled` / `57014` before the stream was dropped, so the consumer flag did not rewrite a user-request cancel. `prove_late_cancel` recorded `not_sent then sent`: the finished generation refused, and the in-flight generation accepted. `prove_drop_stream` dropped an unlocked query whose complete hook fails: `partial`, `end_audit_ok=false`, socket dropped.

The 2000 ms walls on this fixture took the ready server-error arm. They did not take the SQLSTATE-absent client-sleep arm. That arm is `Hot::Deadline` in `await_hot`. The contract still describes it. This oracle does not claim to have timed it.

## F4. Role bind timing

`role_bind_decision` returns `Invalid` when the shape check fails or the role string differs from `identity.role`. It returns `Reuse` when a different valid role is already stored. It returns `Proceed` for the first valid role and for the same valid role again. `Invalid` does not set or clear the stored role. A limit error returns before this decision.

`prove_fresh_guard_binds`: `INSERT` is `GuardError::Write`, the session is open, `bound_role` is `oo_r_reader_a`, and the audit count is unchanged. The other valid role is `RoleReuse` with the audit count still unchanged. The same role then returns 4 rows, status `succeeded`. Oracle: `fresh_guard_bind=other_role_rejected same_role_ran`.

`prove_invalid_role_does_not_bind`: shape `not-a-role` is `RoleParameter` and leaves `bound_role` empty. A role string that does not equal `identity.role` does the same. `max_rows` 2001 is `Limit` and leaves the role empty. After a successful bind, the invalid shape is `RoleParameter` again and the stored role stays. Oracle: `invalid_role=not_bound`.

`prove_audit_fail`: admit failure is `AuditUnavailable`, the user function call count is unchanged, the session stays open, and `bound_role` is already set. The other valid role is `RoleReuse` with the audit count unchanged. Oracle: `audit_fail=bound_then_other_role_refused`.

`prove_login_flag_closes`: `oo_r_flag_probe` is `LOGIN`. The shape check binds it. The `pg_roles` read inside the transaction returns `RoleParameter`, the socket is dropped, and the stored role remains. The next call, including a different role, is `SessionClosed`. The audit count moves by the admit row only. Oracle: `flag_role=session_closed`. The flag check is still required before the user statement.

The library test `role_bind_is_permanent_for_a_valid_role_only` checks the same decision table and checks that 750 ms is below 1 s and below the 30 s wall.

## F5. What the deciding oracle also kept

`live/oracle.txt` on the repaired executor, same PostgreSQL 17.10, TLS SAN `DNS:oo-a189-pg`, `track_functions=pl`:

- `untrusted_admission=blocked`
- star: 4 rows, `succeeded`
- row bound: 2 rows, 3 calls, truncated
- byte cap: 1 row, 4 bytes, truncated
- oversized: 0 rows, truncated, `succeeded`
- `role_reuse=refused` after a successful bind
- `guard_refusals=hidden extra write`
- `type_refused=oid:1700 calls_delta:0`
- `lying_extra=42501 failed`
- `role_b=rows:4 cols:1`
- direct grants: hidden and extra columns `42501`, role B visible `ok:4`
- `note_oid=16669` and `fixture.page_hold` are fixture catalog entries

The fixture OID and the volatile `page_hold` function are the live test catalog. They are not a docs rule R4 whitelist. The audit hook is a second connection's autocommit insert. It is not a production audit WAL.

## F6. Preserved failures

- `linux/probe/attempt-1/`: baseline harness, `repro.exit` 101, panic `locked query never became active`. Portal and role facts were already written. The lock drop was not measured on that attempt.
- `linux/proof/attempt-1/`: `proof.exit` 101, `lib-test.exit` 101, rustc `E0106` missing lifetime on `open_read_only` once `HotWait` was a second borrow. The later signature gives the transaction the client lifetime only. One compiler error was reported.
- `linux/proof/attempt-2/`: library tests 11 passed, then `proof.exit` 2 because clippy was not installed. The smoke had downloaded clippy into a container layer that was removed with that container. The deciding `inside.sh` installs the component when it is absent. `linux/proof/clippy-component.log` starts with `info: downloading component clippy`.

No assertion in the deciding live test was removed to obtain the green oracle.

## F7. What stays open

- the 500 ms process-isolated parser worker, authentication, and any untrusted admission path
- a production audit WAL
- row-level security, claims, and row filters
- a connection pool
- result OIDs outside bool, int2, int4, int8, text, and null
- docs rule R4 as a generated function whitelist
- parent SP06-W04, SP06-W05, and SP06-W06

`driver.abort` on `close` still ends the connection driver after the socket slot is cleared. That is session teardown. It is not the cancel used for a dropped portal. The dropped-portal path is `cancel_query` on the stored backend identity, then a continued poll.
