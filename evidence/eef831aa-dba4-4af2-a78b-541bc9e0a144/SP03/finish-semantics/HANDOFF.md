# Handoff — a144 finish semantics

Coder result on fixed source `6b95d6f2fd2259edfcbffc27a6338518a64dd0fc`. Owned commit `f5f0a73b0678e8b1611f1dc4a40e1dbf1d67b10e`. Independent review of that commit is next. R4 and the full reader stay unsigned. Do not mark the ledger.

## Read these

1. `FINDINGS.md` — why `finish` returns the budget error after a skipped `ROLLBACK`, and why `snapshot.rs`, `session.rs`, and `stream.rs` stayed at the fixed blobs.
2. `linux/probe-2/finish-outcome.txt` — drained generation 1 `SnapshotFinished` ids `1,2,3`; generation 2 same ids; cancel fact `cancelled`; bound finish fact `max_value`; pid 156 `reader_rollback_lines=0` `reader_reset_lines=1`; SQL verbs with zero DML, DDL, `COMMIT`, and `pg_terminate_backend`.
3. `linux/probe-2/postgres-window.log` lines 84–87 (pid 150 and 151 `ROLLBACK`, then reset), lines 244–247 (cancel `ROLLBACK`, then reset), lines 323–328 (pid 156 `SAVEPOINT`, the wide `SELECT`, reset, `unexpected EOF` on an open transaction, then pid 157 `ROLLBACK`).
4. `linux/probe-1/console.log` and `linux/probe-1/pg_reader_finish_live.exit` — compile exit 101, preserved. Blob `b22ae52fc75b036cd0d8008be31fb3207dbe228d`.
5. `linux/smoke/lib.exit` and `clippy.exit` — both 0, recorded before probe-1. Lib result `63 passed`.
6. `contracts/source/pgsql/reader-lifecycle-v1.md` paragraph under Caller, added in `f5f0a73b`. The live overlay hashed the previous lifecycle blob `0f08c66c16333f68ac30719fc4b631b0baf8c2b9`.

## What the probe showed

A drained EOF finishes. An explicit cancel does not finish. A real `max_value` portal fault does not finish, and the data socket can reset with no `ROLLBACK` while both backends are gone within 5000 ms. The same committed ids come back on the next generation.

## Left for the reviewer

Confirm the three product blobs in `f5f0a73b` are still `f1a71fa73064dfb3b83d7d0183f43c6e1a9751a7`, `7e191eacbf61439680cbd92a7109ded6c8a05596`, and `ba9b215a059440a4681578200e7d719cf22407e2`. Confirm probe-2 `archive_hashes_match=1` against `linux/probe-2/expect-*.sha256`. The 5000 ms drain, guard-cancel failure, and unsynchronized shutdown were not provoked. `pg_reader_drop_live` and `pg_reader_cancel_live` were not rerun, because those sources were not edited.

## Cleanup

After the evidence commit, remove only this task's containers, volumes, network, and `/tmp/oo-a144-eef831aa-dba4-4af2-a78b-541bc9e0a144`. Commands are in `commands.md`. Do not prune and do not remove other owners' containers.
