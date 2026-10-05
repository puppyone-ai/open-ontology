# Handoff — a216

## Result

Verdict: **APPROVE_LIMITED**.

Independent review of `8434b451b597177a4a8b60f9d5031dc73cbf14bf`. Parent `99b25ff3d8cfefe11484017e3e31cfb6d3c9e4db`. Tree `b3f85cd335ab3b1f77d44a5dc6371422ef04a0db`. Author run a210 is `31767bbcbdf7dadad390f604d17d923c50329d22`. The reviewed request is a205 `cd1e7492b20fc0dd1c0ac0261ac547ebfeace671`.

`linux/smoke/smoke.exit` is 0 on the immutable archive before the overlay. `linux/proof/proof.exit` is 0. Lib tests passed 20 in both runs, including the before-transport release test, the after-transport fail-close test, and the leftover empty-versus-held test. Clippy `-D warnings` exited 0 in both runs. `live_cancel_raii_review` passed in 4.06s. The full `live_permission_bound_select` matrix was not repeated.

`linux/proof/oracle.txt`:

```text
before_transport=join:aborted clone:not_sent during:busy holds:0 read:ok rows:1 idle:ok open:yes next:started gen_before:1 gen_at_release:1 unexpected:0
owner_abort=join:aborted row:row idle:ok open:no during:busy clone:not_sent first:closed after_1200ms:closed retry:not_sent backend:gone codes:sslrequest holds:1 unexpected:0
abort_then_drop=join:aborted idle:ok drop_ms:0 open:no status:partial rollback:no sqlstate:none clone:not_sent next:closed backend:present codes:sslrequest holds:1
leftover=alive_at_hold:true alive_after_idle:false join:timeout-none idle:ok drop_ms:740 open:no next:closed backend:present codes:sslrequest,sslrequest holds:2
```

Release before transport kept generation 1 and the next execute `started`. Abort after a held SSLRequest made the next execute `closed`, including 1200 ms later, where a205 had recorded `busy`. The non-owning clone stayed `not_sent`.

`abort_then_drop` is partial, rollback unconfirmed, backend present. `owner_abort` `backend:gone` is the activity read after the brief row and the session close. The live leftover future returned timeout before idle ended; the library leftover test is the isolated check. `UNTRUSTED_ADMISSION` stays `Blocked`. SP06-W04, SP06-W05, and SP06-W06 stay open.

Failed proof attempts remain:

- `linux/proof/attempt-1` exit 101. Armed proxy during `drop(stream)`.
- `linux/proof/attempt-2` exit 101. Consumer drop closed the session after a real release.
- `linux/proof/attempt-3` exit 101. `before_transport` had already passed. `review_brief` was not visible in `pg_stat_activity`.

Each has `why-stopped.txt`. Product sources were not edited.

## Do next

A later slice can read `linux/proof/oracle.txt` and `query_executor.rs` from `ReservationGuard` through `settle_lend`. Leave public admission `Blocked`. Do not add an HTTP route, a pool, or an untrusted statement entry from this review. Do not treat `backend:gone` on `owner_abort` as proof that PostgreSQL stopped.

## Review

Grok 4.7 in this session was the only coder, tester, and reviewer. No subagent and no worktree. One Rust 1.94 builder at a time, one PostgreSQL 17.10. New builders were 6 GiB and 1 CPU. Smoke found `oo-a215-preoverlay` already running and left it alone. Global cap in the launch script is 10.

This review does not approve a release and does not mark a parent item done.

## State left behind

`cleanup.txt` records removal of this task's containers, volumes, network, and private temp directory. Pinned images stay. `docker prune` was not used. Other tasks' containers stay.

`SHA256SUMS` lists the other files in this directory. `SHA256SUMS.sha256` is the digest of that list. Both were written by `shasum -a 256`.
