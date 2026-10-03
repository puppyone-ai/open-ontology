# FINDINGS — a141 blocked owner Drop

UUID `eef831aa-dba4-4af2-a78b-541bc9e0a141`. Fixed input `58afa97ded85a7dd89c72532bfc8973c45255d7c`, tree `543fd103532a8366ce92a0b4eaea4e089eb76d2d`. Source commit `6b95d6f2fd2259edfcbffc27a6338518a64dd0fc`.

a139 keys stay approved. a139 cancel and unpublished-run stay REQUEST_CHANGES on `58afa97`. This session repairs the owner-Drop absence failure those findings named. R4, the full reader, and the SP03 parent stay unsigned. No ledger checkbox was marked.

## Cause

`PgSession::shutdown` on `58afa97` rolled back and then called `Connection::close`. That close sends `Terminate` and waits for a TLS `close_notify`. On the blocked-owner Drop path the server log shows `pg_cancel_backend`, SQLSTATE `57014`, `ROLLBACK TO SAVEPOINT`, `RELEASE SAVEPOINT`, and `ROLLBACK`, and the graceful close does not finish. `linux/red-1` has no reader `could not receive data from client` during the 5000 ms window. The original `(pid, backend_start)` rows stay until the test process dies.

Dropping the session after that rollback closes the TCP socket. `linux/green-3/postgres-window.log` at `2026-10-03 22:22:46.437` records `ROLLBACK` and then `could not receive data from client: Connection reset by peer` for data pid 2386 and guard pid 2387. The same observation, after `pg_stat_clear_snapshot()`, reports both rows absent at 54 ms (`linux/green-3/drop-outcome.txt`). `owner_drop_finish_called=0` and `snapshot_finished=0`.

`linux/green-1` and `linux/diag-2` still panicked at 5000 ms with those pids listed, `wait` empty, and `xact=true`. `linux/diag-2/stacks.txt` is an autocommit sampler on a different connection: the `oo-pg-source` rows are present at `22:12:55.644` and `22:12:55.774`, the reset is logged at `22:12:55.842`, and the rows are absent from `22:12:55.906` onward. The fixture admin holds `BEGIN` plus `LOCK TABLE ... ACCESS EXCLUSIVE` for the whole observation. PostgreSQL 17.10 default `stats_fetch_consistency=cache` keeps the first `pg_stat_activity` copy until commit, abort, or `pg_stat_clear_snapshot()`. `wait_event` is read live, so it goes empty after the backend exits while the frozen copy still says `active`. The regression calls `pg_stat_clear_snapshot()` on `oo-fixture-admin` before each poll. That statement is fixture SQL. The 5000 ms bound is unchanged.

## Change

`SnapshotOwner::Drop` sets the cancel flag, spawns stop plus both releases, and returns. The spawn is wrapped in `SOCKET_ABORT_AFTER` (1000 ms). That constant bounds the spawned rollback. It is not a second absence deadline. Drop does not return `SnapshotFinished`. With no runtime handle, the sessions drop on the calling thread.

`PgSession::shutdown` rolls back when the connection can take a command, then drops the socket. A skipped rollback drops the socket and returns `Ok`. A desynchronized connection or a poisoned named portal drops the socket and returns `cleanup_unconfirmed`. A rollback that finishes inside 1000 ms stays confirmed. A rollback that does not finish returns `cleanup_unconfirmed`. The socket drop is the session value going out of scope. It is not `pg_terminate_backend` and it opens no third session. The existing read-only `pg_cancel_backend` guard is unchanged. `stream.rs` is unchanged from `58afa97`.

`contracts/source/pgsql/reader-lifecycle-v1.md` records that Drop returns when cleanup is scheduled, that an uncertain protocol drops the socket, and that the poll is a current read. `CLEANUP_OBSERVATION_BOUND_MS` stays 5000.

## Observation

`linux/green-3` drop, on the refreshed poll: data pid 2386 start `2026-10-03 22:22:46.121704+00` and guard pid 2387 start `2026-10-03 22:22:46.241161+00` were active at the first sample (1 ms) and absent at 54 ms. Reader audit: 41 statements, 1 `pg_cancel_backend` select, verbs `RELEASE=1,ROLLBACK=3,SAVEPOINT=1,SELECT=34,START=2`, zero DML, DDL, COMMIT, or `pg_terminate_backend`.

`linux/green-3` cancel, once: `receiving_fact=cancelled`, `finish_after_request_fact=cancelled`, deadline 19771 ms, killed next fact SQLSTATE `57P01` does not finish, fresh generation 9, 555 reader statements, 8 cancels, 1 writer write, zero DML, DDL, COMMIT, or OFFSET.

`linux/green-3` keys, once: collation `af-NA-x-icu`, sparse chunk matches the cut, late keys stay out of the old cut, `nokey_generation=3`, 995 reader statements, 2 writer writes, zero DML, DDL, COMMIT, or OFFSET.

`linux/clippy-1` is the same overlay hashes as `linux/green-3`. `cargo clippy --manifest-path Cargo.toml --locked --offline -p oo-connectors --all-targets -- -D warnings` exited 0 on `rustc 1.94.0` `aarch64-unknown-linux-gnu`. The pinned image has no clippy component, so this run added it the same way `linux/smoke` did. The two `sqlx-postgres` unfulfilled `#[expect(dead_code)]` warnings are the known dependency warnings and did not fail the gate. `linux/green-3` container exit is 1 because that run invoked clippy before the component existed. Its three test exits are 0. That directory was not overwritten.

## Still open

A drain past 5000 ms and SQLSTATE `25006` were not provoked. `cleanup_unconfirmed` stays the fact for a desynchronized or portal-poisoned shutdown. The column-revoke matrix was not re-run. Independent review of `6b95d6f2` is next. This handoff does not accept the full reader.
