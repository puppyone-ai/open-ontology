# Findings — a205

Verdict: **REQUEST_CHANGES**.

Reviewed source is `0ee7cf86ad443ee1d84fe9fe3ed3f7d8847799c1`. Line numbers are `source/blobs/crates/oo-gateway/src/query_executor.rs`. Measurements are `live/oracle.txt`, which matches `linux/proof/oracle.txt`. The PostgreSQL server was 17.10 over private TLS. Backend absence is a `pg_stat_activity` read by pid and `backend_start`.

The stall proxy in the review harness accepts a connection, reads the 8-byte startup prefix, and holds the socket only when it is armed and the code is SSLRequest (`80877103`) or cleartext CancelRequest (`80877102`). Every other prefix is forwarded. The builder mount is `ca.crt` plus the two password files. It does not mount `server.key`.

## 1. Aborting a reserved cancel future leaves SessionBusy after the producer has exited

`live/oracle.txt`:

```text
aborted_success=join:aborted row:row idle:ok idle_ms:7 open:no status:partial rollback:yes socket:yes sqlstate:none first:busy after_1200ms:busy retry:not_sent after_retry:busy backend:gone codes:sslrequest holds:1 forwarded:2 unexpected:0
```

The handle reserved the generation. The proxy held one SSLRequest and forwarded nothing unexpected. The future was aborted while that hold stood. The row was then read, the stream was dropped, and `wait_idle` returned in 7 ms, so the producer had exited. The stored summary is `partial`, SQLSTATE absent, rollback confirmed, socket dropped. The later activity read was `backend:gone` after that socket drop. The summary does not claim the backend was gone.

The next `execute_trusted_select` was `SessionBusy`. It was still `SessionBusy` 1200 ms later, after the 750 ms cleanup bound. A retry `cancel` returned `NotSent` and left the following execute `SessionBusy`.

Contract, "Cancel, failure, reuse": once the reserved transport exceeds the cleanup deadline or returns a transport error, the session is fail-closed and the next call is `SessionClosed` once the producer has exited. A handle that does not own the reservation does not fail-close it. This run's producer has exited and the session is not open, and the next call is still `SessionBusy`.

`CancelHandle` is `Clone` (line 275) and has no `Drop`. `cancel` (lines 877–929) calls `try_reserve_cancel` before any await (line 878). It calls `release_reservation` only on the not-sent path (lines 904–907). It calls `fail_close_session` on an observe error (lines 886–889), a zero remainder (lines 911–913), a transport error (lines 921–923), and its own timeout (lines 925–927). Aborting the future skips all four of those arms, so `cancel_reserved` stays set.

`execute_trusted_select` returns `SessionBusy` when `cancel_reserved` is set (lines 639–641) before it looks at `open` (lines 642–644). `shutdown_local` (lines 754–759) is the only closer that clears the reservation, and it runs on `close` and `DedicatedSession` drop. A caller that retries on `SessionBusy` stays in that state until the session value itself is dropped.

A `Drop` on `CancelHandle` would be the wrong guard: the session keeps another clone. The future that called `try_reserve_cancel` has to release the reservation if `cancel_query` was not entered, and fail-close the session if the transport was entered. This observation is the entered-transport case (the proxy already held SSLRequest).

## 2. Abort, then drop the in-flight stream, still reports Busy

`live/oracle.txt`:

```text
aborted_drop=join:aborted idle:ok drop_ms:727 open:no status:partial rollback:no sqlstate:none next:busy backend:present codes:sslrequest,sslrequest holds:2
```

The same abort ran while `page_hold` was in flight, and the proxy stayed armed. Drop returned in 727 ms. That is the one shared deadline, already started by the handle, spent by the producer cancel as well. The proxy held two SSLRequest sockets. The summary is `partial`, stop class from the consumer path, SQLSTATE absent, rollback not confirmed, and the same backend row was still present. The session was not open. The next execute was `SessionBusy`.

`send_bound_cancel` (lines 1399–1419) returns `Expired` on timeout and does not clear `cancel_reserved` or call `fail_close_session`. `settle_lend` returns immediately when `reuse` is false (lines 2188–2190) and does not clear the reservation. `wait_cancel_settled` returns when the remainder is zero (lines 2181–2182) with the reservation still set. The producer then drops the client and sets `open = false` (lines 1064–1065). `end_generation` (lines 2058–2061) bumps the generation and clears `accepting_cancel`, and leaves `cancel_reserved`. The execute check at lines 639–641 therefore reports `SessionBusy` on a closed session.

No wrong-generation side effect was observed. The retry in section 1 was `NotSent`. The stuck session was not lent. Section 4 released a held cancel prefix onto a later pid and that pid stayed active.

## 3. The completed-future stall path meets the bound

These rows are the same campaign, with the cancel future left to run:

```text
stalled_drop=ms:752 status:partial rollback:no backend:present slo:miss accepts:1
stalled_handle=ms:754 busy:yes status:succeeded closed:true rollback:yes
fresh_deadline_drop=ms:752 status:partial rollback:no sqlstate:none stop:protocol open:no next:closed backend:present codes:sslrequest slo:miss
```

A stall of only the cancel transport returns inside 700–1000 ms. Rollback is not claimed. The activity row is still present, so the run does not call the backend gone. Expiry is an SLO miss. The next call after the producer exits on that path is `SessionClosed`. While the handle's own future is still inside its timeout, the next execute is `SessionBusy`; after that future hits its timeout arm (lines 925–927), `closed:true` and the socket is dropped with rollback confirmed on the short query that was allowed to finish.

`CLEANUP_BOUND` is 750 ms (line 100). Two successful `SELECT`s on one proxied session stayed open (`deadline_reset_q1=succeeded open`, `deadline_reset_q2=succeeded open`). The following stall-drop was 752 ms, so `begin_generation` (lines 2048–2055) had cleared `cleanup_deadline`. The next cleanup did not inherit a spent budget.

## 4. Late packet, healthy drop, and the fences that held

```text
late_packet=released:1 next_active:true old_backend:present
healthy_after_reset=ms:47 status:partial rollback:yes sqlstate:none stop:protocol backend_gone:true
lock_drop_ms=6 lock_status=partial lock_sqlstate=none lock_stop=protocol lock_rollback=yes
sleep_drop_ms=6 sleep_status=partial sleep_sqlstate=none sleep_stop=protocol sleep_rollback=yes
whole_page_budget=sqlstate:none elapsed_ms:3044 rollback:no
deadline=57014 elapsed_ms:1787
inflight_cancel=sent 57014
client_budget=57014 elapsed_ms:1789 late:not_sent
finished_drop_next=still_active drop_ms:2
role_reuse=refused
invalid_role=not_bound
flag_role=session_closed
live_result=compared
```

Releasing the held cancel bytes after a new session's `page_hold` was active left that new backend active. The old backend row was still present. `mark_cancel_packet` (lines 2134–2138) clears the reservation and records `cancel_packet_sent`; `may_lend` (lines 2082–2084) refuses a lend when that flag is set. The packet that was held did not become a cancel of the next pid.

A healthy drop after that reset returned in 47 ms with rollback confirmed and the activity row absent. The lock-blocked drop and the `pg_sleep` drop each returned in 6 ms, `partial`, SQLSTATE absent, rollback confirmed. The whole-page client budget fired at 3044 ms with SQLSTATE absent and rollback not claimed, so the cleanup deadline did not replace the `Instant` taken in `execute_trusted_select`. Server timer, in-flight user cancel, and the client-budget case stayed SQLSTATE `57014`, distinct from the drop summaries. A finished stream's drop left the next hold active. Role reuse was refused, an invalid role did not bind, and the flag-role failure closed the session.

`live_result=compared` means the author campaign embedded in the overlay matched its expected rows, including the permission, truncation, audit, and guard checks.

## 5. Repair that would clear this verdict

Keep the reserve-before-await fence. When the future that owns the reservation is dropped before `cancel_query` is entered, call `release_reservation`. When it is dropped after the transport is entered, call `fail_close_session`. On producer exit, `settle_lend` and the `!reuse` path must fail-close a reservation that is still set when the shared remainder is zero or the producer is leaving. After that, `execute_trusted_select` on the closed session returns `SessionClosed`. A second handle that does not own the reservation stays `NotSent` and is not the releaser.

This review did not change product source.
