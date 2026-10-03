# Findings — a144 finish semantics

The reachable public paths on `6b95d6f2fd2259edfcbffc27a6338518a64dd0fc` do not turn an aborted data socket into `SnapshotFinished`. No edit to `snapshot.rs`, `session.rs`, or `stream.rs` was required. The live proof is `linux/probe-2/finish-outcome.txt` and `linux/probe-2/postgres-window.log`.

## What finish returns

`SnapshotOwner::finish` (`snapshot.rs` 570–609) takes the pull task first. A pull error poisons the owner. If the cancel flag is set, `finish_cancel` runs and the function returns `protocol` fact `cancelled` unless rollback is uncertain. With the flag clear it builds `SnapshotFinished`, calls `release_both`, and returns a budget pull error immediately (`snapshot.rs` 599–601). Any other pull error is the poisoned `early` value and is returned after `release_both`. `Ok(SnapshotFinished)` runs only when the pull returned success, the owner was not already poisoned, the catalog recheck and deadline hold, and `release_both` returns `Ok`.

`PgSession::shutdown` (`session.rs` 722–727) returns `Ok` when `skip_rollback` is set, after dropping the socket. That `Ok` is what a143 flagged. It becomes `SnapshotFinished` only when `finish` has no pull error and no other `early` error.

## The max_value path

`tests/pg_reader_finish_live.rs` opens `r3f.wide` (`note` text `abcdefghij`, 10 bytes) with `StreamBudget::try_new(1, 1_048_576, 4, 1024)`. `next_batch` returned `budget` fact `max_value`. `finish` returned the same fact. Data pid 156 logged `SAVEPOINT oo_stream_lock` (line 323), `SELECT id,note FROM r3f.wide` (line 324), `could not receive data from client: Connection reset by peer` (line 325), and `unexpected EOF on client connection with an open transaction` (line 326). It logged no `ROLLBACK`. Guard pid 157 did log `ROLLBACK` (line 327). Both `(pid, backend_start)` pairs were absent from the first current stat sample after `finish` returned (0 ms, bound 5000 ms). `finish` waits for `shutdown` before it returns, so the sample can already be empty. The reset line shows the data backend had been connected and then lost the socket.

That is the production state a143 described, reached through a real receive bound rather than a fake enum: portal fault, skip `ROLLBACK`, `shutdown` `Ok`, both sockets released. The caller still got the budget error.

The scan arm is `Hold::Fail` (`stream.rs` 626–628). `close_owned` sees the poisoned portal and returns `protocol` fact `portal_poison`. `settle_after` (`stream.rs` 1076–1081) marks skip-rollback and returns `Ok` for that fact. `prefer` (`stream.rs` 1108–1116) always returns `Err`, and it keeps a budget error. The pull task therefore ends in `Err`. `finish` cannot take the `Ok(SnapshotFinished)` arm.

## Why scan success plus skip-rollback was not patched

Every other `mark_skip_rollback` in `scan` returns `Err` from `scan`: a failed portal open (`stream.rs` 610–616), `rollback_only` wrapped by `prefer`, and `signal_cancel` (`stream.rs` 901–916). `Hold::Stop` and `Hold::Done` call `close_owned` only after `fill_batch` returns a row or EOF. A successful `pull` leaves the portal `Suspended`, or `Closed` with the lease `Clear`. `close_owned` returns `portal_poison` only when `connection_poisoned` is already true, so those two arms do not enter the swallow. A later close error is not fact `portal_poison`, and `settle_after` returns that error rather than `Ok`. `Signal::Stop` before a portal is open (`stream.rs` 559) returns `Ok` without having set `skip_rollback`.

A clear flag and a skipped rollback would produce `SnapshotFinished` only if `scan` returned `Ok` while `skip_rollback` was set. That pair is not reachable from these arms. A `finish` check of `rollback_skipped`, or changing `shutdown` to return an error on skip, would not fire on a path this probe can reach. Changing `shutdown` would also change `abort`, which treats `release_both` `Ok` as `SnapshotAborted` even when the owner was poisoned. Those edits were not made.

A caller that stops after a confirmed portal close and `ROLLBACK` can still receive `SnapshotFinished`. That is the drained EOF in this probe (pid 150 logged `ROLLBACK` at `postgres-window.log` line 84, then the reset at line 85) and the existing mid-stream close covered by `pg_stream_bounds`. Socket exit after a confirmed `ROLLBACK` is not treated as a failed read. Socket exit without `ROLLBACK` on the `max_value` path is.

`cleanup_unconfirmed` for a drain past 5000 ms, a guard-cancel error, or an unsynchronized shutdown was read and not provoked. Those arms return `Err` from `scan` or from `shutdown` before `SnapshotFinished`.

## Preserved failure

Probe-1 exit 101 is a test compile failure. `StreamItem` does not implement `Debug`, so `Result::expect_err` could not name the success value. The raw console is `linux/probe-1/console.log`. The draft blob is `linux/probe-1/finish-test-blob.txt` (`b22ae52fc75b036cd0d8008be31fb3207dbe228d`). Probe-2 matches on `Ok(StreamItem::...)` arms. The product archive hash did not change between the two probes.
