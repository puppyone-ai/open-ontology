# Findings — a210

Reviewed the committed repair `8434b451b597177a4a8b60f9d5031dc73cbf14bf` against a205 `cd1e7492` and the Linux oracles in this directory. Grok 4.7 in this session was the only coder, tester, and reviewer. No subagent and no worktree. This is the repair check. It is not an independent approval and it does not close a parent item.

## 1. The unfixed executor still leaves the reservation set

`linux/red/oracle.txt`, archive executor `2ad166e91a407f5bff39d814d7beb4a1bc69710e30851260528184f3e8558925`:

```
aborted_success=join:aborted row:row idle:ok idle_ms:487 open:no status:succeeded rollback:yes socket:yes sqlstate:none during:busy clone:not_sent during_again:busy first:busy after_1200ms:busy retry:not_sent after_retry:busy backend:gone codes:sslrequest holds:1 forwarded:1 unexpected:0
aborted_drop=join:aborted idle:ok drop_ms:722 open:no status:partial rollback:no sqlstate:none clone:not_sent next:busy backend:present codes:sslrequest holds:2
```

`linux/red/live.log` panics with `owning cancel abort left busy after_1200ms:busy; aborted drop left busy`. Raw `linux/red/red.exit` is 101. Container `2b295af90229816d74828bcfd6676beca6af33f2e6b239202c6daf64e5f65471`. The proxy had already held `SSLRequest` (`codes:sslrequest`). The non-owning clone stayed `not_sent` and the session stayed `busy` while that future was alive. After abort, the producer had exited (`idle:ok`, `open:no`) and the next execute was still `busy` at once and after 1200 ms. Retry cancel stayed `not_sent`. The drop path returned in 722 ms with rollback unconfirmed, the backend row present, and the next call `busy`.

`linux/red/attempt-1` exit 101 is a compile error (`E0499`) in the overlaid test. The abort assertion was not reached. `linux/red/attempt-1/why-stopped.txt` says so. It is not the product red.

## 2. The repaired future clears its own reservation

`linux/proof/oracle.txt`, source SHA-256 `7604e5ee02bd493c5cc03e355c4c98f0c69f36bd601a07dcb6b76b67153df094`:

```
aborted_success=join:aborted row:row idle:ok idle_ms:0 open:no status:succeeded rollback:yes socket:yes sqlstate:none during:busy clone:not_sent during_again:busy first:closed after_1200ms:closed retry:not_sent after_retry:closed backend:gone codes:sslrequest holds:1 forwarded:1 unexpected:0
aborted_drop=join:aborted idle:ok drop_ms:0 open:no status:partial rollback:no sqlstate:none clone:not_sent next:closed backend:present codes:sslrequest holds:1
```

Raw `linux/proof/proof.exit` is 0. Container `633d3ac31c0645d3d898ea895fb236c2e64cb761082ed5215f6bf3a506c471c7`. `live_permission_bound_select` passed in 22.92s. Lib tests passed 20, including:

- `reservation_guard_drop_before_transport_releases_only_its_generation`
- `reservation_guard_drop_after_transport_fail_closes_only_its_generation`
- `producer_leftover_reservation_fail_closes_and_an_empty_one_does_not`

The success path still reads `row` while the cancel transport is held, the clone stays `not_sent`, and a page started during that hold stays `busy`. After abort, the first execute and the execute after 1200 ms are `closed`. The drop path's next execute is `closed`. `unexpected` stays 0. Held sockets are discarded. They are not written onto a later backend.

The same proof oracle still has `stalled_drop=ms:761 status:partial rollback:no backend:present slo:miss accepts:1` and `stalled_handle=ms:752 busy:yes status:succeeded closed:true rollback:yes`, plus `deadline=57014 elapsed_ms:1791`, `inflight_cancel=sent 57014`, and `late_cancel=not_sent then sent`.

## 3. Review of the committed guard

Checked on `8434b451b597177a4a8b60f9d5031dc73cbf14bf`:

- `try_reserve_cancel` still runs before `observe_within`. The 750 ms deadline is unchanged.
- `ReservationGuard` is local to the future. There is no `impl Drop for CancelHandle`.
- `release_own_reservation` and `fail_close_owned` return immediately when `cancel_reserved` is not `Some(this generation)`. A `NotSent` clone constructs no guard.
- `enter_transport` is synchronous and sits immediately before `timeout(cancel_query)`. The live tests abort only after the proxy has counted an `SSLRequest`, which is inside that await.
- The success arm disarms before `mark_cancel_packet`. Those two calls have no await between them.
- `close_leftover_reservation` fail-closes only when a reservation is still set, under one lock. An empty reservation leaves the session open, which is the path the existing stall-drop case already uses when no handle reserved.
- `fail_close_owned` and `close_leftover_reservation` do not await. Destroy is `watch::Sender::send(true)`, the same call `shutdown_local` already uses. The producer holds the receiver. Tokio 1.53.1 does not store `send` when `receiver_count` is 0, which is why the unit helper keeps its receiver.
- The pre-edit `fail_close_session` callers were all inside `cancel`. Those arms now set the guard fate and let `Drop` settle the reservation. No remaining call site was left without a generation check.
- `query_executor.rs` has no `unsafe`. Clippy `-D warnings` on `--lib --test query_executor_pg_live` exited 0 (`linux/proof/clippy.log`, 1.33s). The proof hashes match `source/blob-sha256.txt`.

## 4. Attempts that are not the product result

- Smoke exit 0 is the unchanged archive. 17 lib tests. Clippy finished in 23.00s. `linux/smoke/overlay-absent.txt` says `overlay=absent`.
- Proof attempt-1 ran the smoke library because a file mounted over the archive kept the mtime cargo had recorded. `cargo clean -p oo-gateway` is in the proof script so rustc opens the mounted source. `linux/proof/phases/lib-guard.exit` is 0 only when the new unit test name is in the log.
- Proof attempt-2 is the dropped-receiver unit failure described above. The live campaign did not run.

## 5. Limits that remain

`aborted_drop` on the repaired executor is `status:partial`, `rollback:no`, `backend:present`, `sqlstate:none`. The session is closed. The backend row was still present. This repair does not prove the PostgreSQL backend stopped.

`aborted_success` reaches `status:succeeded` because `fixture.abort_brief` returned its row while the cancel transport was held. The activity read then reports `backend:gone`. That is the observation after the short query finished.

The live abort is after `SSLRequest` is held. Release before `cancel_query` is the unit test, which drops a guard of another generation as a no-op and drops the owning guard before `enter_transport`.

SP06-W04, SP06-W05, and SP06-W06 stay open. `UNTRUSTED_ADMISSION` stays `Blocked`. There is no HTTP route, pool, or untrusted statement entry in this commit. No dependency or lock file changed.
