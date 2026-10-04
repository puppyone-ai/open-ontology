# PostgreSQL run cancel and unpublished-run failure

This is the active-pull cancel and the failure close on one already-open snapshot owner. Acceptance is qualified to the PostgreSQL full-mode snapshot, the bounded pull, and `RunComplete` admission. Resume cursors, CDC, and the registry API stay unsigned. Catalog `type_support.converted` stays false. `connector_capabilities` `scan` and `bounded_read`, and type capability `bounded_reader`, are supported for that path. Key planning stays `reader-keys-v1`. The pull stays `bounded-stream-v1`. `SnapshotOwner::finish` returns `SnapshotFinished` as lifecycle close only. That value is not admission. `SnapshotOwner::finish_complete` returns `RunComplete` only after this owner delivered `StreamItem::Exhausted` for every selected table.

## Caller

`SnapshotOwner::run_cancel` returns a `RunCancel` that shares the owner's flag. `RunCancel::request` only sets that flag. It does not roll back, close, or prove the backend has exited.

`SnapshotOwner::cancel` sets the flag, stops the pull, then rolls back and closes both sessions. `RunCancelled` is returned only when neither session skipped `ROLLBACK` and the pull did not end `cleanup_unconfirmed`. `RunCancelled` is not `SnapshotFinished`. The generation stays consumed. Its output is incomplete. The loader discards it.

`finish` after the flag is set releases both sessions and returns `protocol` fact `cancelled`. It does not return `SnapshotFinished`.

With the flag clear, `finish` still requires the pull task to return success, the catalog recheck to pass, both releases to succeed, an unpoisoned owner, and the deadline. A portal value past `max_value` fails the pull with `budget` fact `max_value`. `settle_after` can skip `ROLLBACK` after `portal_poison` and return success to `prefer`, and `prefer` keeps that budget error. `finish` returns the same fact. `shutdown` drops the data socket without `ROLLBACK`. That reset is not `SnapshotFinished`. Both backends still leave a current `pg_stat_activity` sample within `CLEANUP_OBSERVATION_BOUND_MS`. A drained portal EOF that rolls back and closes still returns `SnapshotFinished`. That value is lifecycle release for compatibility. It does not require `StreamItem::Exhausted`, and a successful rollback is not proof that every selected table was read. A later generation of those committed rows reads the same ids. `RunCancel::request` before `finish` still returns `protocol` fact `cancelled`.

`finish_complete` consumes the owner. It checks cancellation, the deadline, and the current catalog and ACL before release, then closes both connections. It returns `RunComplete` only when this owner delivered `StreamItem::Exhausted` for every selected table and those release checks pass. The pull records that delivery when the consumer takes `Exhausted` from a scan of every selected table. `TableEof` does not record it. A key-range or single-table chunk does not record it. Callers cannot construct `RunComplete` and cannot assert completion. The token binds the source id, config identity, generation, type policy, input digest, fixed catalog, and selected-table identity captured for this owner. No stream, a stream that was not pulled, one batch while rows remain, and a later selected table that was never scanned return `protocol` fact `not_exhausted` after both connections close. Cancellation, a budget or portal or source failure, a deadline, a current catalog or ACL failure, and a failed cleanup return that failure and do not mint `RunComplete`. No production target admits a load yet. Only `RunComplete` may admit an eventual loader. `SnapshotFinished` cannot.

`abort` is unchanged: it releases and returns `SnapshotAborted` when cleanup succeeds, including when the owner was already poisoned.

Dropping the `next_batch` future puts the drive back. The in-flight portal open or Execute keeps running. That drop is not backend exit. `cancel`, `finish`, `abort`, or `Drop` of the owner is what stops it.

`Drop` of the owner sets the flag and schedules stop, rollback, and close on the current Tokio runtime. `Drop` does not return `SnapshotFinished`. There is no third connection and no `pg_terminate_backend` on the reader path.

## In-flight portal

The data session opens the named scan portal (Parse and Bind) and then writes Execute. A lock wait can sit in that Parse before any Execute. The guard session runs one statement:

```sql
SELECT pg_catalog.pg_cancel_backend(a.pid)
FROM pg_catalog.pg_stat_activity a
WHERE a.pid = $1::int4 AND a.backend_start::text = $2
LIMIT 1
```

The parameters are the data backend pid and `backend_start::text` captured at open. No row means that backend is already gone. A reused pid is not cancelled. The pull polls that same portal open or Execute first. A 50ms sleep rechecks the flag, the client deadline, and the statement timeout. That future is not dropped when one of those fires.

The guard then runs the cancel statement. The pull keeps polling that same future until it returns, or until `CLEANUP_OBSERVATION_BOUND_MS` (5000) passes. `SQLSTATE 57014` is classified by the reason that was set: the flag is `protocol` fact `cancelled`, the client deadline is `budget` fact `deadline`, and the statement timeout is `budget` fact `statement_timeout`. Any other driver error keeps its existing mapping.

If the drain exceeds 5000ms, or the guard cancel fails (including `SQLSTATE 25006` from the read-only guard transaction), the data connection is marked to skip `ROLLBACK`, the fact is `protocol` `cleanup_unconfirmed`, and the socket is closed. The owner does not open another session to finish the cancel.

A dropped client timeout on catalog `fetch_all` / `fetch_one` is unchanged. It still does not cancel the backend. Only the scan portal open and Execute path above does.

## Unpublished run

`SnapshotFinished` is the successful lifecycle close and is not admission. `RunComplete` from `finish_complete` is the only value that may admit an eventual loader. This slice still has no production target admission. A cut that lost its source, hit a deadline, was cancelled, failed a statement, or did not deliver `StreamItem::Exhausted` for every selected table does not return `RunComplete` and does not reconnect under the same generation. Later chunks are not appended to that output.

A new `SnapshotOwner` generation after a committed writer mutation reads that new cut completely. It does not mix rows from the old generation. The old output stays incomplete.

`SNAPSHOT_DEADLINE_DEFAULT` is 300 seconds. The caller passes the `Instant`. This owner does not start that clock and does not raise the `PgBudget` statement or lock cap. Those stay at most 120 seconds. A fixture may pass a shorter deadline. A deadline longer than the statement cap does not extend the server `statement_timeout`.

## Release observation

After `cancel`, `finish`, `abort`, or `Drop` returns, an admin poll of `pg_stat_activity` for that `(pid, backend_start::text)` should find no row within `CLEANUP_OBSERVATION_BOUND_MS`. The row must not remain `active` or `idle in transaction`. The product connection cannot observe its own idle state. A confirmation query on that backend would be `active`.

`Drop` returns when that cleanup is scheduled. It does not wait for the backends to exit, and it does not return `SnapshotFinished`. The 5000ms bound is the absence deadline measured from that return. It is not added to the statement timeout or to a TLS close. `ROLLBACK` is attempted when the connection can take a command. If that attempt does not finish, or the protocol is uncertain (`cleanup_unconfirmed`, a poisoned portal, or a failed guard cancel), the owner drops the TCP socket instead of waiting for a graceful `Terminate` or a TLS close_notify. Dropping the socket is not `pg_terminate_backend` and does not open another session. A `ROLLBACK` that completed stays confirmed. An aborted socket is not `SnapshotFinished`.

The poll is a current read of backend status. PostgreSQL's default `stats_fetch_consistency=cache` keeps the first `pg_stat_activity` copy for the rest of the observer transaction, so a copy taken while the backends are still alive stays visible after they exit. `pg_stat_clear_snapshot()` discards that copy. The 5000ms bound applies to the current read.
