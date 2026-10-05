# Findings — a216

Verdict: **APPROVE_LIMITED**.

Reviewed commit `8434b451b597177a4a8b60f9d5031dc73cbf14bf`. Parent `99b25ff3d8cfefe11484017e3e31cfb6d3c9e4db`. Tree `b3f85cd335ab3b1f77d44a5dc6371422ef04a0db`. Compared with a210 reports at `31767bbcbdf7dadad390f604d17d923c50329d22` and a205 `cd1e7492b20fc0dd1c0ac0261ac547ebfeace671` (REQUEST_CHANGES on `0ee7cf86ad443ee1d84fe9fe3ed3f7d8847799c1`).

Grok 4.7 in this session was the only coder, tester, and reviewer. No subagent and no worktree. Line numbers below are the frozen `query_executor.rs` in `source/blobs/`. Measurements are `linux/proof/oracle.txt`, which matches `live/oracle.txt`. PostgreSQL was 17.10 over private TLS. Backend absence is a `pg_stat_activity` read by pid and `backend_start`.

The review proxy accepts a connection, reads the 8-byte startup prefix, and holds the socket only when it is armed and the code is SSLRequest (`80877103`) or cleartext CancelRequest (`80877102`). Every other prefix while armed increments `unexpected`. The builder mount is `ca.crt` plus the two password files. It does not mount `server.key`.

## 1. Release before transport leaves the session usable

`linux/proof/oracle.txt`:

```text
before_transport=join:aborted clone:not_sent during:busy holds:0 read:ok rows:1 idle:ok open:yes next:started gen_before:1 gen_at_release:1 unexpected:0
```

The select waited on an exclusive lock of the shadow table. The owning cancel future was aborted while observation was paused, so `cancel_query` was not entered (`holds:0`). The generation captured after the stream started was still 1 immediately after abort. A second execute during that hold was `busy`. The non-owning clone returned `not_sent`. After the lock was released, the select returned one row, the producer went idle, the session stayed open, and the next execute was `started`.

`release_own_reservation` (line 2194) clears `cancel_reserved` only when it is `Some(this generation)` and does not clear `open`. `reservation_guard_drop_before_transport_releases_only_its_generation` passed in `linux/smoke` and again in `linux/proof` (20 tests each). That test drops a guard for generation 9 while generation 4 is reserved, and the reservation stays.

Attempts 1 and 2 are harness failures of this same case, preserved with `why-stopped.txt`. Attempt 1 dropped the stream while the proxy was armed, so producer cleanup stalled and the session closed (`next:closed`, `holds:0`, generation unchanged). Attempt 2 disarmed first. The next execute was still `closed`. The frozen `prove_drop_stream` path records a consumer drop as partial, socket dropped, and a closed session. `publish_stop` sets `reuse` false, and the producer then sets `open` false (lines 1077–1081). Those two runs do not show a stuck reservation.

## 2. Abort after SSLRequest is Closed once the producer has exited

```text
owner_abort=join:aborted row:row idle:ok open:no during:busy clone:not_sent first:closed after_1200ms:closed retry:not_sent backend:gone codes:sslrequest holds:1 unexpected:0
abort_then_drop=join:aborted idle:ok drop_ms:0 open:no status:partial rollback:no sqlstate:none clone:not_sent next:closed backend:present codes:sslrequest holds:1
```

Both cases held one SSLRequest. The clone stayed `not_sent`. A page started while the owning future was alive stayed `busy`. After abort, `wait_idle` returned and the next execute was `closed`. The owner-abort execute was still `closed` after 1200 ms. Retry cancel stayed `not_sent`. `unexpected` stayed 0.

a205 recorded `first:busy` and `after_1200ms:busy` on the unfixed executor, and `next:busy` for abort-then-drop. `execute_trusted_select` returns `SessionBusy` when `cancel_reserved` is set (line 643) before it returns `SessionClosed` for a closed session (line 646). `closed` after the producer has exited means the reservation was cleared and the session was fail-closed.

`abort_then_drop` is the honest limit from a205 and a210: status `partial`, rollback unconfirmed, SQLSTATE absent, backend row present, `drop_ms:0`, one SSLRequest. `owner_abort` reads `backend:gone` after `fixture.review_brief()` returned its row and the session closed. That is the activity label. It does not show that cancel stopped the backend.

`reservation_guard_drop_after_transport_fail_closes_only_its_generation` passed in both lib runs. A foreign generation that has entered transport leaves the owner's reservation and `open` flag alone. The owning generation fail-closes. The live after-transport lines do not print the generation. That library test is the correct-generation check.

Attempt 3 passed `before_transport` and then missed `review_brief` in `pg_stat_activity` because the function returns 4 with no sleep. The follow-up holds the table until SSLRequest is held, then releases it so the row can arrive during that hold. Preserved. It is not a product failure.

## 3. Leftover reservation

```text
leftover=alive_at_hold:true alive_after_idle:false join:timeout-none idle:ok drop_ms:740 open:no next:closed backend:present codes:sslrequest,sslrequest holds:2
```

The owning future was alive when the first SSLRequest was held. By the end of `wait_idle` it had finished. `join:timeout-none` means that future returned `ExecError::Timeout { sqlstate: None }`. After `enter_transport` (line 931) the timeout arm (line 944) returns that error with fate already `FailClose`, so `Drop` calls `fail_close_owned` (line 2206). The producer wait was 740 ms and the proxy recorded two SSLRequest holds, so producer cleanup also met the armed proxy. The next execute was `closed`, and the backend row was present.

This live case shows a closed session after a stalled cancel. It does not keep the owning future alive across producer exit, so it does not by itself separate leftover close from that future's timeout `Drop`. `producer_leftover_reservation_fail_closes_and_an_empty_one_does_not` does. An empty reservation stays open. A reservation that is still set fail-closes under one lock (`close_leftover_reservation`, line 2223). That test passed in smoke and in this proof.

`drop_ms:740` sits next to the 750 ms bound. `fence.txt` on the proof run records `cleanup_bound=104:const CLEANUP_BOUND: Duration = Duration::from_millis(750);` and `cancel_handle_drop=absent`. `cleanup_budget_does_not_grow_after_it_starts` passed. This review did not run the `live_permission_bound_select` matrix.

## 4. Healthy cancel smoke and admission

```text
healthy_select=rows:1 status:succeeded rollback:yes open:yes
untrusted_admission=blocked
review_result=compared
```

One existing select completed, confirmed rollback, and left the session open. The oracle records admission `blocked`. No HTTP route, pool, or untrusted statement entry was added.

## 5. Scope

Raw `linux/smoke/smoke.exit` is 0. Raw `linux/proof/proof.exit` is 0. Attempt exits are 101 and remain under `linux/proof/attempt-1`, `attempt-2`, and `attempt-3`. No manifest, lock, or ledger file was edited. The worktree's other dirty paths were left untouched. The commit for this review is the evidence directory only.
