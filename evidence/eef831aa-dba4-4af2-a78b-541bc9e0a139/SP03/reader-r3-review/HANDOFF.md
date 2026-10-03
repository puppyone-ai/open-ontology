# HANDOFF — a139 fixed reader R3 review

Source under review: `58afa97ded85a7dd89c72532bfc8973c45255d7c`, tree `543fd103532a8366ce92a0b4eaea4e089eb76d2d`, parent `8bda0cd48976b23d268d125b34bc8130d94b8253`.

## Verdicts

Keys slice: APPROVE.

Cancel and unpublished-run slice: REQUEST_CHANGES.

R4, the whole full reader, and the SP03 parent stay unsigned.

## Read first

1. `FINDINGS.md` — the drop failure, the key oracle, and the requirements that were not exercised.
2. `REPORT.md` — images, exits, and attempt layout.
3. `linux/author/keys-outcome.txt` and `linux/author/cancel-outcome.txt` — author tests, once each, exit 0.
4. `linux/probe-3/console.log` and `linux/probe-3/review-outcome.txt` — owner `Drop` still has both backends in a transaction at 5000 ms.
5. `linux/probe-3/sql-audit-offline.txt` — reader SQL for that probe window.
6. Contracts on the fixed commit: `contracts/source/pgsql/reader-keys-v1.md`, `reader-lifecycle-v1.md`, `bounded-stream-v1.md`.

## Change requested

`SnapshotOwner::Drop` (`crates/oo-connectors/src/snapshot.rs`) must leave the data and guard `(pid, backend_start)` rows absent within `CLEANUP_OBSERVATION_BOUND_MS` after `Drop` returns. Reproducer: lock a selected table `ACCESS EXCLUSIVE`, `open_stream`, poll `next_batch` until the data backend is `active`, drop that future, drop the owner, poll `pg_stat_activity`. Observed on `58afa97`: `pg_cancel_backend` and `ROLLBACK` are logged, and the same backends are still `active` and `idle in transaction` at 5000 ms. `SnapshotOwner::cancel` after a dropped `next_batch` did release in the author test. That path is not the failing one.

## Still open

A drain past 5000 ms and SQLSTATE `25006` were inspected in `await_cancellable` and `finish_cancel` and were not provoked. `cleanup_unconfirmed` stays the typed poisoned fact for that path.

The column-revoke matrix was not re-run. `snapshot.rs` and `stream.rs` changed after the a137 receipt. Passing reads here used the current pull and returned Arrow batches. The revoke matrix is not re-signed.

a138's overwritten failure logs are still missing. This review did not rewrite them.

No ledger checkboxes were marked. No push and no release.

## Cleanup and manifest

`cleanup.txt` lists the owned containers, volumes, network, and temporary paths removed after the runs. `hashes.sha256` lists every other file in this directory. The evidence commit is path-limited to this directory.
