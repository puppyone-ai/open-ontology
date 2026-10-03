# Handoff — a143 owner Drop / cancel acceptance

Verdict: **APPROVE LIMITED** for the R3 cancel and unpublished-run slice on `6b95d6f2fd2259edfcbffc27a6338518a64dd0fc`.

Keys remain **APPROVE** from a139. `pg_reader_keys_live` passed once on this shutdown (`linux/probe-1/keys-outcome.txt`). R4 and the full reader remain unsigned. Do not mark the ledger.

## Read these

1. `FINDINGS.md` — verdict, the 55 ms current-stat drop, the one old-blob run, and the residual `finish` path.
2. `linux/probe-1/drop-outcome.txt` — pid 128 `2026-10-03 22:49:03.633228+00` and pid 129 `2026-10-03 22:49:03.75325+00` present at 1 ms with live waits, absent at 55 ms, `snapshot_finished=0`.
3. `linux/probe-1/postgres-window.log` lines 88–103 — cancel, 57014, `ROLLBACK`, refreshed `pg_stat_clear_snapshot()`. No reader `could not receive` line in this file. a141 green-3 did log reader resets; do not paste that sentence onto this window.
4. `linux/probe-1/cancel-outcome.txt` and `keys-outcome.txt` — both `*_live_complete`, exits 0.
5. `linux/smoke/lib.exit` and `clippy.exit` — both 0, recorded before the probes.
6. `linux/old-1/drop-outcome.txt` — one current-observer run of the `58afa97` product plus the fixed drop test. Absent at 58 ms. Preserve a139 `linux/probe-3` and a141 `linux/red-1` beside it. The stale-cache signature and the red-1 close hang are different observations. This old-1 pass is not a revert request.

## Acceptance that was met

Owner `Drop` after a blocked `next_batch`, measured by `pg_stat_clear_snapshot()` on the lock-holding admin connection: both original `(pid, backend_start)` rows absent within 5000 ms (observed 55 ms). `Drop` did not return `SnapshotFinished`. Reader SQL on that test was nonempty, with one `pg_cancel_backend` select and zero DML, DDL, `COMMIT`, or `pg_terminate_backend`. Guard cancel stayed a `SELECT`. Admin `ACCESS EXCLUSIVE` and fixture DDL stayed on the admin session.

## Left unsigned on purpose

`SnapshotOwner::finish` (`snapshot.rs` 570–609) can still return `SnapshotFinished` when the cancel flag is clear, `settle_after` has swallowed `portal_poison` into `Ok` (`stream.rs` 1076–1081), and `shutdown` returns `Ok` on `skip_rollback` (`session.rs` 723–727). Not driven live. Not the a139 reproducer.

`cleanup_unconfirmed` for a drain past 5000 ms, a guard-cancel error, or an unsynchronized / portal-poisoned shutdown was read and not provoked. The no-runtime `Drop` arm was read and not run. The 1000 ms spawn timeout was not forced to fire. No column-revoke matrix.

## Do not rerun to tidy

`linux/old-1/drop-test-blob.txt` records `base_had_drop_blob` as the failed `rev-parse` revision string. The overlay blob matched `cbfbcd26f6a223e148b18fc56302af1a76c9dbfb`. Leave the file.
