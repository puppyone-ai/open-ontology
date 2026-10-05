# Handoff — a205

## Result

Independent review of the fixed cancellation transport bounds. Verdict **REQUEST_CHANGES**.

Source `0ee7cf86ad443ee1d84fe9fe3ed3f7d8847799c1`. Parent `39d2851cd70b2591768d85025550dde0924bf81a`. Tree `358029744eb8755834953fdda7bf6cfaf44edf9e`. Author evidence `c989ccf50b54e007d4a8ce118d7a624038a77ba9`. Prior review evidence `65169f374c665d8efdd62a8bcd47317c38a1359e`.

Aborting a reserved `CancelHandle::cancel` leaves `cancel_reserved` set. After the producer exits, the session is not open and the next execute is still `SessionBusy` at once and again after 1200 ms. A retry cancel is `NotSent` and the execute stays `SessionBusy`. Abort then drop of an in-flight stream returns in 727 ms, rollback not confirmed, backend row present, next call `Busy`. `FINDINGS.md` sections 1 and 2 quote the oracle lines and the source lines.

The completed-future stall path holds: 752 ms, `partial`, rollback not claimed, backend present, SLO miss, next call `SessionClosed`. The handle timeout is 754 ms and then closes. Deadline reset, late-packet fence, healthy rollback, whole-page client wall, `57014` versus SQLSTATE-absent drops, and role bind all held on the same oracle. `live_result=compared`.

## Do next

Repair the reservation owner so a dropped cancel future releases or fail-closes, and so producer exit fail-closes a leftover reservation. The next call on that closed session is `SessionClosed`. Do not put the `Drop` on `CancelHandle`: it is `Clone`, and the session keeps a clone. A handle that does not own the reservation stays `NotSent`.

Leave SP06-W04, SP06-W05, and SP06-W06 open. Leave the public admission `Blocked`. Do not add an HTTP route, a pool, or an untrusted statement entry from this review.

Read `live/oracle.txt` and `query_executor.rs` lines 639–644, 754–759, 877–929, 1061–1066, 1399–1419, 2058–2061, and 2109–2206 before editing. The 752 ms stall-drop path already meets the shared 750 ms bound. The gap is future abort, not that bound.

## Review

Grok 4.7 in this session was the only coder, tester, and reviewer. No subagent and no worktree. One Rust 1.94 builder at a time, one PostgreSQL 17.10. Pre-overlay smoke `linux/smoke/smoke.exit` is 0. Live phase `linux/proof/phases/live.exit` is 0. Raw `linux/proof/proof.exit` is 2 because the fresh container had no clippy component (`clippy component missing`). Follow-up `linux/proof/phases/clippy.exit` is 0 on the same overlay bytes. Failed smoke and proof attempts remain under `linux/smoke/attempt-*` and `linux/proof/attempt-*`.

This review does not approve a release and does not mark a parent item done.

## State left behind

`cleanup.txt` records removal of `oo-a205-pg`, volumes `oo-a205-cargo`, `oo-a205-target`, and `oo-a205-pgdata`, network `oo-a205-net`, and `/tmp/oo-a205-eef831aa-dba4-4af2-a78b-541bc9e0a205`. Smoke, proof, and clippy containers were already removed by their launch scripts. Pinned images stay. `docker prune` was not used. Other tasks' containers stay.

`SHA256SUMS` lists the other files in this directory. `SHA256SUMS.sha256` is the digest of that list. Both were written by `shasum -a 256`.
