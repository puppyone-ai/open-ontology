# Findings — a143 owner Drop / cancel acceptance

Reviewer: Grok 4.7 xhigh, sole independent reviewer. No subagents.

Source under review: `6b95d6f2fd2259edfcbffc27a6338518a64dd0fc` (parent `6cdb559b45ea9d0fed709ca74d80eecf264d8016`, subject `connectors: drop the reader socket after bounded rollback`). Tree `baaabaf1384646237e0c40bfa1914ea150d39c5b`. The diff against that parent is four files: `crates/oo-connectors/src/session.rs`, `crates/oo-connectors/src/snapshot.rs`, `crates/oo-connectors/tests/pg_reader_drop_live.rs`, `contracts/source/pgsql/reader-lifecycle-v1.md` (649 insertions, 23 deletions). `crates/oo-connectors/src/stream.rs` blob `ba9b215a059440a4681578200e7d719cf22407e2` is identical at `58afa97ded85a7dd89c72532bfc8973c45255d7c` and at `6b95d6f2`. Cancel test blob `10e9b7f3c261b4b0a3b03c8dce27c0ed7c101355` and keys test blob `6185746a8de512cea416b58055dc9e21e8c905c2` are likewise identical. Workspace HEAD stayed `c83184f2f930435e17dd7c87d06b9d8ce7cc1554` through every archive.

## Verdict

Cancel and unpublished-run slice: **APPROVE LIMITED**.

Keys stay **APPROVE** from a139. This run executed `pg_reader_keys_live` once on the fixed shutdown; it passed. That is a regression check, not a new keys signature.

R4 and the full reader stay unsigned. The residual `finish` path below stays unsigned. No ledger checkbox is marked.

## Fixed drop, current stats

`linux/probe-1` is a git archive of `6b95d6f2` with no overlay. `pg_reader_drop_live` exit 0, finished in 0.48s. Outcome `linux/probe-1/drop-outcome.txt`:

- Admin held `ACCESS EXCLUSIVE`. `open_stream`, then `next_batch` polled until the data backend was active, then that future was dropped: `next_batch_drop_state=active data_pid=128 reader_backends=2`.
- First current sample at 1 ms, after `pg_stat_clear_snapshot()`, still had both original rows and live waits: pid 128, `backend_start` `2026-10-03 22:49:03.633228+00`, state `active`, wait `Lock/relation`, `xact=true`, query `SELECT id,note FROM r3d.held`; pid 129, `backend_start` `2026-10-03 22:49:03.75325+00`, state `idle in transaction`, wait `Client/ClientRead`, `xact=true`.
- `owner_drop_absent_ms=55`. Both original `(pid, backend_start)` rows were absent. Bound is `CLEANUP_OBSERVATION_BOUND_MS` = 5000.
- `owner_drop_finish_called=0` and `snapshot_finished=0`. `Drop` did not return `SnapshotFinished`.
- Reader SQL audit: `oo_statements=41`, `cancel_selects=1`, `dml_ddl_commit_terminate=0`, verbs `RELEASE=1,ROLLBACK=3,SAVEPOINT=1,SELECT=34,START=2`.

Server window `linux/probe-1/postgres-window.log` lines 88–103 (drop, before the cancel fixture at line 107): guard pid 129 ran `SELECT pg_catalog.pg_cancel_backend(...)` for pid 128; pid 128 logged `canceling statement due to user request` on `SELECT id,note FROM r3d.held`, then `ROLLBACK TO SAVEPOINT oo_stream_lock`, `RELEASE SAVEPOINT oo_stream_lock`, and `ROLLBACK`; pid 129 logged `ROLLBACK` at `22:49:03.946`. The refreshed `pg_stat_clear_snapshot()` poll is at `22:49:03.986`. The only `could not receive data from client: Connection reset by peer` line in that drop window is admin pid 127 at `22:49:03.991`. The whole 1553-line probe window has three such lines, all fixture-admin pids (127, 130, 151). It has no reader `could not receive` line and no `unexpected EOF` line. `log_disconnections` was not enabled. Absence for this run is the current `pg_stat_activity` sample, not a reset line.

a141 `linux/green-3/postgres-window.log` on the same fixed shutdown did log `Connection reset by peer` for reader pids 2386 and 2387 at `22:22:46.437`, immediately after their `ROLLBACK`, with absence at 54 ms. This run's 55 ms absence stands on its own sample. The missing reset line is recorded; it is not rewritten into a141's wording.

## One corrected observer on the old blob

`linux/old-1` archived `58afa97` and overlaid only `pg_reader_drop_live.rs` from `6b95d6f2` (blob `cbfbcd26f6a223e148b18fc56302af1a76c9dbfb` on both the fixed commit and the snap). Session content sha `f969c8f0bf2e068ac3ba10560d15215adf69460227e559fee717a352324e14d0` (base). Snapshot content sha `c00cf3871af9a86788e80e78c8895742cd232045a72f509783b56f551d49c9b9`. Stream content sha matches the fixed archive. The test calls `pg_stat_clear_snapshot()`. Exit 0, 0.47s. `linux/old-1/drop-outcome.txt`: first sample at 1 ms, pid 118 `2026-10-03 22:46:43.614654+00` `active` wait `Lock/relation`, pid 119 `2026-10-03 22:46:43.732646+00` `idle in transaction` wait `Client/ClientRead`; `owner_drop_absent_ms=58`; `snapshot_finished=0`; same reader verb audit as the fixed drop. The 103-line window shows the cancel, SQLSTATE 57014, savepoint rollback, `RELEASE`, and both `ROLLBACK`s. The only reset line is admin pid 117. That is the expected server trace when `Connection::close` finishes: `Terminate` then TLS `close_notify`, then the backend exits, and with `log_disconnections` off there is no `could not receive` line.

`linux/old-1/drop-test-blob.txt` line `base_had_drop_blob=` prints the revision string `58afa97...:crates/oo-connectors/tests/pg_reader_drop_live.rs` because `git rev-parse` wrote that name before failing. The file is absent on `58afa97`. The overlay hash check passed. The console was not rewritten.

This single old run shows the pre-fix graceful close can leave both original rows absent inside 5000 ms when the observer calls `pg_stat_clear_snapshot()`. It does not replace a139 or a141 red-1, and it is not a reason to revert the socket drop. The committed contract on `6b95d6f2` is the bounded rollback plus a socket drop, and the fixed probe met that contract on a current read.

## a139 and a141 red-1, left as they are

a139 `linux/probe-3` on `58afa97` reported both original rows still `active` / `idle in transaction` at 5000 ms after owner `Drop`, with `pg_cancel_backend`, 57014, and `ROLLBACK` already in the server log. That review's observer transaction did not call `pg_stat_clear_snapshot()`. PostgreSQL 17 default `stats_fetch_consistency=cache` keeps the first `pg_stat_activity` copy for the observer transaction. Frozen wait and state strings across a long poll are the stale-copy signature. The contract text added in this fix says the 5000 ms bound applies to a current read.

a141 `linux/red-1` is a separate observation on the old shutdown: no reader `could not receive` line during the 5000 ms window, and the original rows still listed at the end of that window. A completed `Terminate` removes the row. A row that is still present on a later sample, together with no reader disconnect line for the whole window, is the graceful close still in progress. That hang did not reproduce on this review's one corrected old-1 run. Both records stay. Stale cached stats are not backend persistence. A missing disconnect line over a window where the current sample still shows the row is persistence.

## What the fixed Drop and shutdown do

`SnapshotOwner::drop` (`snapshot.rs` 1204–1245) sets `released`, stores the cancel flag, and returns without a `SnapshotFinished` value. With a Tokio handle it spawns `slot.stop()` plus `release_session` for both sessions, or `release_session` for the data and guard sessions, inside `tokio::time::timeout(SOCKET_ABORT_AFTER)`. `SOCKET_ABORT_AFTER` is 1000 ms (`session.rs` 43). The spawn does not block `Drop`. The live drop was absent at 55 ms, inside that spawn budget and inside 5000 ms. This review did not force the 1000 ms timeout to elapse.

`release_session` (`snapshot.rs` 2125–2127) is `session.shutdown()`. Fixed `PgSession::shutdown` (`session.rs` 722–742):

- `skip_rollback`: drop the permit and the session, return `Ok(())`.
- `!incoming_synchronized()` or `named_portal_poisoned()`: drop the socket, return `protocol` fact `cleanup_unconfirmed`.
- Otherwise `timeout(SOCKET_ABORT_AFTER, finish(Ok(())))`, then always `drop(self)`. Timeout elapsed returns `cleanup_unconfirmed`. `finish` on the session runs `ROLLBACK` when `in_transaction` (`session.rs` 744–765).

`drop(self)` drops `PgConnection` / `PgStream`. `Connection::close` (`vendor/sqlx-postgres/src/connection/mod.rs` 288–300) is the path that sends `Terminate` and then `shutdown` (TLS `close_notify` in `poll_shutdown`). The fixed shutdown does not call `Connection::close`. It does not call `pg_terminate_backend`. `close_notify` is sent from `poll_shutdown`, not from a `Drop` of `RustlsSocket`.

No-runtime path, inspected, not run live. If `Handle::try_current()` fails while a stream slot is taken, the slot is not spawned; it drops at the end of that block. `StreamSlot` (`stream.rs` 369–373) holds the drive future and has no `Drop` impl, so dropping it drops `run()`, which holds both `PgSession`s, which drops the sockets on the calling thread without `shutdown()`. If there is no stream slot and no runtime, `Drop` calls `drop(data)` and `drop(guard)` on the calling thread (`snapshot.rs` 1243–1244). The measured probe had a runtime, so it took the spawn path.

## Explicit cancel, and the unprovoked poisoned branch

`pg_reader_cancel_live` exit 0, 22.69s. `linux/probe-1/cancel-outcome.txt`: `dropped_future_backend_still_active`, then `drop_then_cancel_generation=4`, `after_complete_generation=5`, `deadline_fact=deadline elapsed_ms=19767`, `finish_after_request_fact=cancelled`, `killed_next_fact=Some("database") sqlstate=Some("57P01")` (the test's own kill, not a reader `pg_terminate_backend`), `old_run_not_finished`, `sql_audit oo_statements=432 cancels=5 writer_writes=1 dml_ddl_commit_offset=0`, `cancel_live_complete`.

`pg_reader_keys_live` exit 0, 1.14s. `keys-outcome.txt`: `keys_live_complete`, `oo_statements=872`, `writer_writes=2`, `dml_ddl_commit_offset=0`.

Inspected, not provoked live:

- Drain past 5000 ms in `await_cancellable` returns `cleanup_unconfirmed` (`stream.rs` 883–884). `stream.rs` is outside this diff.
- Guard cancel failure, including a read-only `25006`, hits `signal_cancel`'s error arm: `mark_skip_rollback` and `cleanup_unconfirmed` (`stream.rs` 901–916). There is no literal `25006` match in `crates/oo-connectors/src`; any `fetch_one` error other than `row_not_found` takes that arm. A pid that does not fit in `i32` takes the same fact at 902–904.
- `finish_cancel` (`snapshot.rs` 727–746) returns `cleanup_unconfirmed` when `rollback_skipped()` is set or the stream/poison fact is already `cleanup_unconfirmed`, after `release_both`.
- Unsynchronized or portal-poisoned `shutdown` returns `cleanup_unconfirmed` after the socket drop (`session.rs` 729–732).

## Residual, not part of this approval

`SnapshotOwner::finish` (`snapshot.rs` 570–609) consults `rollback_skipped()` only through `finish_cancel`, and `finish_cancel` runs only when the cancel flag is set. With the flag clear it calls `release_both` and, on `Ok`, can return `Ok(SnapshotFinished)`. `settle_after` (`stream.rs` 1076–1081, unchanged) turns a `portal_poison` close into `Ok(())` after `mark_skip_rollback`. Fixed `shutdown` then returns `Ok(())` on `skip_rollback` (`session.rs` 723–726). The old `shutdown` still called `Connection::close` and mapped a close error to `cleanup_failed`. The contract line added in this commit says an aborted socket is not `SnapshotFinished`. This review did not drive that path. It is not the a139 reproducer. It is not signed.

## Package smoke before probes

`linux/smoke` archived `6b95d6f2` and ran before either probe. The probe launchers refuse to start unless `linux/smoke/lib.exit` and `clippy.exit` are 0. Both are 0. `cargo test --locked --offline -p oo-connectors --lib`: 63 passed, 0 failed, 0.00s. `cargo clippy --locked --offline -p oo-connectors --all-targets -- -D warnings`: exit 0, dev profile finished in 11.03s. Two unfulfilled `#[expect(dead_code)]` warnings in vendored `sqlx-postgres` (`flush.rs:11`, `message/mod.rs:76`) did not fail `-D warnings` on `oo-connectors`. Clippy was installed first (`clippy-component.exit` 0). a141 green-3 container exit 1, from invoking clippy before the component existed, stays that task's record and is not a product failure here.

`rust-toolchain.toml` at `6b95d6f2` says channel `1.98.1`. The containers set `RUSTUP_TOOLCHAIN=1.94.0` and exit 2 unless `rustc` is `1.94.0` host `aarch64-unknown-linux-gnu`. All three consoles record `rustc 1.94.0 (4a4ef493e 2026-03-02)` host `aarch64-unknown-linux-gnu`.

Lib coverage reused for Arrow / ACL is the unit tests in that 63, including `export_arrow_fixture`, masked-column rejects, and the password-omission config tests. No live column-revoke matrix was run.
