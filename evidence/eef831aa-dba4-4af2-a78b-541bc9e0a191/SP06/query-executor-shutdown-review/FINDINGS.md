# Findings — a191

Verdict: **APPROVE LIMITED**. The measurements below are from `live/oracle.txt` on PostgreSQL 17.10 over the dedicated TLS session. Source lines are `source/blobs/query_executor.rs` at `15d23fe9f2cbc74e3f3278d70d0f56390568126e`.

## 1. Reachable-server shutdown

Both drops used wall 8000 ms and were polled active for 5200 ms before `Drop`.

| Case | Before drop | Cleanup | Backend | Summary |
|---|---|---|---|---|
| `ACCESS EXCLUSIVE` on `fixture.gate` | `state=active`, `wait_event_type=Lock`, `wait_event=relation` | 10 ms | gone | `partial`, `Protocol`, SQLSTATE absent, rollback confirmed, end audit true |
| `pg_sleep` via `fixture.a191_hold` | `state=active`, `wait_event_type=Timeout`, `wait_event=PgSleep` | 9 ms | gone | same |

Backend absence is `COUNT(*)` from `pg_stat_activity` where `pid` and `to_char(backend_start AT TIME ZONE 'UTC', 'YYYYMMDDHH24MISSUS')` both match the bound values. `socket=true` is recorded beside that count. A fresh admin connection reread the audit table at the same count (`durable_reread=29`, `same_session=29`). The latest complete row for each drop was `status=partial`, `rollback_confirmed=true`, SQLSTATE absent. `xact_rollback` increased by 1 on each; that counter is supporting evidence, and the row plus the activity count are the proof.

a189's repaired oracle, which this review did not need to repeat as a campaign, recorded the same shape at 3 ms and 6 ms after 3100 ms still active. a189's baseline lock drop on the earlier tree waited 5703 ms and completed `failed` / `57014`. The fixed tree still does that under a reachable server, now with the activity check.

A finished stream's drop left the next hold active (`finished_drop_next present=true state=active wait=Timeout`). The following drop of that next statement was 7 ms, backend gone, `partial`, rollback confirmed.

## 2. Four stops stay distinct

`classify_pg` (`query_executor.rs` 1786-1811) maps SQLSTATE `57014` whose message starts with `canceling statement due to user request` to `Canceled`, and any other `57014` to `Timeout` while keeping the SQLSTATE. Every call site passes `deadline_owned=false` (lines 1233, 1368, 1485). The client-deadline arm does not use `classify_pg`. `deadline_stop` (1156-1164) sets `Timeout { sqlstate: None }`, `desync=true`, `partial=false`.

A consumed response while shutdown is already set becomes `consumer_stop` even when the server error was `57014` (`hot_result` 1230-1232). Status `partial`, stop `Protocol`, SQLSTATE absent. `cleanup` then uses `rollback_bounded` at 750 ms (1269-1270).

Measured:

- Server timer, wall 2500 ms, lock held: `server_57014_ms=2289`, `err=timeout-sqlstate`, `sqlstate=57014`, `status=failed`, `stop=timeout`, `rollback=true`, `end_audit=true`. Display omitted SQL and the password.
- User cancel: `outcome=sent`, `err=canceled`, `sqlstate=57014`, `status=failed`, `stop=canceled`, `rollback=true`. The stream was read before it was dropped, so the consumer flag did not rewrite the class.
- Client deadline after one portal page: `pace_rows=1`, `pace_first=1`, `pace_error_ms=8047`, `err=timeout-none`, `sqlstate=absent`, `status=failed`, `stop=timeout`, `rollback=false`, backend gone. Wall was 8000 ms. `PACE_SQL` has no `ORDER BY`. `a191_pace(1)` sleeps 6.5 s and `a191_pace` of the second row sleeps 30 s. PostgreSQL restarts `statement_timeout` on the second `Execute`. The client `Instant` taken at `execute_trusted_select` entry (line 614) is the clock that fired. Rollback is not claimed, which matches `desync` dropping the transaction (1264-1267).
- End-hook failure: `end_hook_ms=3`, `status=partial`, `stop=protocol`, `rollback=true`, `end_audit=false`, `admit_delta=1`, `complete_delta=0`, backend gone. `complete` returning before its insert does not rewrite the status. `patch_audit` (1859-1862) writes only `end_audit_ok`.

a189's 2000 ms wall cases completed `failed` / `Timeout` / `57014` at 1796 ms and 1798 ms, and that report says the SQLSTATE-absent arm was not timed. This oracle times it.

## 3. Role bind

One session, in order, from the passing checks behind `live/oracle.txt`:

- Shape `not-a-role` is `RoleParameter` and does not bind. Audit count unchanged.
- Identity `west` with request role `east` is `RoleParameter` and does not bind.
- `max_rows` 2001 is `Limit` and does not bind.
- Wall `Duration::from_micros(1500)` is `Limit` and does not bind. `checked_wall` (1900-1909) requires a whole millisecond from 1 through 30000.
- `INSERT INTO fixture.gate` is `Guard(Write)`, display clean, bound `oo_r_a191_east`, session open, audit delta 0. Bind is stored at lines 635-638 before `guard_select`.
- A later role `west` is `RoleReuse`, display `dedicated session cannot change role`, audit still unchanged, bound role stays `east`.
- The same role `east` then reads `fixture.gate` twice. Both succeed, one row, cell text `7`, rollback confirmed, socket kept, end audit true, session open.
- A separate session bound to `west` reads the same row.

`admit` failure: `AuditUnavailable`, bound `east`, session open, audit delta 0. The other role is again `RoleReuse` with no new audit row. The same role then succeeds once admission works.

Flag role `oo_r_a191_flag` is `LOGIN` and otherwise clear. The failure is inside the read-only transaction, after `admit` and before the user statement (`apply_session` 1496-1506). Result: `RoleParameter`, summary `failed` / `Protocol`, SQLSTATE absent, session closed, bound role remains the flag role, `end_audit=true`, `audit_delta=2` (the admit row and the complete row). The next call with `west` is `SessionClosed` and the audit count stays at that value. `run_producer` drops the client and sets `open=false` unless the attempt is the success reuse path (975-979).

## 4. Cancel identity and the 750 ms clock

`CLEANUP_BOUND` is 750 ms (line 93).

`await_hot` (1183-1217):

- Shutdown already set: `Hot::Consumer`. The database call is not started and `cancel_query` is not sent.
- Shutdown arrives while the future is polled: `send_bound_cancel`, then the same future stays pinned until it is ready or 750 ms passes (`Hot::Abandoned`). The biased select prefers the future over the sleep, so a ready server error beats the cleanup sleep.
- Client deadline: `send_bound_cancel`, then `Hot::Deadline` immediately. The unread future is dropped. That is the pace result above.

`send_bound_cancel` (1241-1255) checks `accepting_cancel`, generation, pid, and `backend_start` under the lock, drops the lock, and then awaits `cancel.token.cancel_query(tls)`. There is no second session and no await between the check and that call. There is also no timeout on the call, and the generation is not read again after the await. A mismatch returns without calling `cancel_query`. `tokio_postgres` `cancel_query` opens a new TCP and TLS connection. The token pid is stable for the life of the connection. `begin_generation` (1865-1870) increments the generation and sets `accepting_cancel`. `end_generation` (1873-1877) increments it again and clears the flag. The generation is the fence the contract names.

`CancelHandle::cancel` (826-846) awaits `observe_backend` on a different session first, then `cancel_gate` (851-871), then `cancel_query` with no timeout. The contract's "no await between that check and the call" holds for the gap between the gate and the call. It does not put a deadline on `cancel_query`.

Hot awaits, each through `await_hot` or `hot_result`: `open_read_only` (`BEGIN`), `platform_batch` (search_path, `SET LOCAL ROLE`, `statement_timeout`), `platform_query` (`pg_roles`), `platform_query_one` twice, `prepare_user`, `bind_portal`, and `read_pages` `query_portal` with page size 1. `push_row` selects on shutdown and does not call `cancel_query`. These are outside the hot poll: `send_bound_cancel`'s `cancel_query`, `CancelHandle`'s observe plus `cancel_query`, `rollback_bounded` (750 ms), `rollback_confirmed` (remaining wall), `same_backend`, audit `admit` and `complete`.

On the drop path the producer task owns the client until `drive` returns. A second `execute_trusted_select` hits `reap_finished` and gets `SessionBusy` while that task is unfinished (768-771). The connection stays with the generation that requested the cancel.

Fault, in-process TCP proxy, TLS passed through, stall accepts new TCP and does not forward it:

- `fault_drop_blocked=true`, `fault_drop_ms=1502`, `fault_drop_state=active`, `fault_drop_active=true`, `fault_cancel_accepts=1`. The original `pg_sleep` backend was still active when the 1500 ms observation fired. The 750 ms poll had not started, because it is sequenced after `cancel_query` returns. Releasing the proxy let the proof finish; the await is unbounded in source, and this run measured the stall window rather than an upper bound.
- `fault_race_held=true`, `fault_race_brief=row`. The 4 s statement finished and returned its row while the cancel TCP was held, so the packet had not aborted that generation. The next hold became active on the same session. After release, `fault_race_next=still-running`, `fault_race_state=active`, and `fault_race_cancel=sent`. The `Sent` join is after the 1500 ms snapshot, so this line shows the next generation alive at that snapshot. It does not show that a late pid-and-secret packet can never arrive during the next statement.

Healthy late-cancel fence, required by contract line 47: `late_cancel finished=not_sent during=not_sent still_state=active`. The live handle then returned `sent` and the stream error was `canceled`.

`docs/17-rust-implementation.md` says a late cancel must not hit a connection returned and lent to the next user, and that exceeding the cleanup limit destroys the connection. This slice has no pool. The drop path waits for `cancel_query` instead of destroying the socket at 750 ms. That matches contract lines 49-51 and is the limit named in `REPORT.md`. A change that aborts `cancel_query` at 750 ms and drops the socket would be a contract change. This review does not request it on the evidence above: the reachable-server requirement held, and the stalled-TCP case did not lend the session to another generation.

## 5. Left as prior proof

a184 (`df6067e9`) already compared typed int2, int4, int8, text, bool, and null, including minima; a star expansion with the hidden column absent; row and byte truncation; type refusal for OIDs 1700 and 1043; a lying catalog `42501`; a guard `INSERT` whose display hides the SQL; admit failure; role reuse after a successful bind; and displays without SQL or passwords. This review cites that result and does not rerun it.

`UNTRUSTED_ADMISSION` is `Blocked` (`query_executor.rs` public constant, checked by the overlay). The fixed `lib.rs` does not export a parser worker, and `parser_worker.rs` is absent from the archive. The 500 ms process-isolated parser, authentication, HTTP, a pool, a production audit WAL, row filters, and parent SP06-W04, SP06-W05, and SP06-W06 stay open, as the contract's own open list says.
