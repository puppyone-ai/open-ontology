# Named portal v1

Driver API on `PgConnection` for one named PostgreSQL portal bound inside a transaction the caller already opened. Connector sessions, Arrow batches, and a source-wide row budget are later slices. This contract does not expose arbitrary SQL on an Open Ontology connector.

## Admission

`open_named_portal` requires `transaction_depth >= 1` and ready-for-query status `T`. Status `E` is an aborted transaction and is refused. A wire `T` with depth 0 is refused. The portal keeps the exclusive connection borrow until completion, `close`, or drop.

Arguments are `PgArguments`. Rows are binary `PgRow` values from the statement metadata captured at prepare. The statement is not inserted into the statement cache. Portal names (`sqlx_p_`) use a counter separate from statement names (`sqlx_s_`). Either counter refuses the open when the next id would wrap.

## Pull

`PgPortalAllowance::positive` rejects zero. Each `pull` reserves exactly one row (`EXECUTE_LIMIT`) from that allowance before it writes `Execute`, even when the caller grants more. The rest of the grant stays with the caller. `reserved_rows` and `consumed_rows` are on the step and on `last_pull`. This reservation is the hook a later connector budget can read. It is not a source-wide limiter.

The driver writes `Execute` with limit 1 and `Flush`. It reads until `PortalSuspended`, `CommandComplete`, `EmptyQueryResponse`, or an error before another `Execute` or a page-end result. A `DataRow` alone is neither suspension nor completion. A byte limit is neither suspension nor completion.

`PortalSuspended` yields `PgPortalOutcome::Suspended` and leaves the portal open. No `Close` and no `Sync` are written between pages. `CommandComplete` yields `Complete` only after the owned portal and statement are closed and `ReadyForQuery` is observed. On PostgreSQL 17.10, an `Execute` limit that returns a row sends `PortalSuspended` even when that row is the last one. The portal completes on a later demanded `Execute` that returns `CommandComplete` and no `DataRow`. The driver does not write that follow-up `Execute` by itself. Until `ReadyForQuery`, PostgreSQL 17.10 reports the backend as `active` in `pg_stat_activity`. A query that returns no row sends `CommandComplete` on the first `Execute`, with `consumed_rows = 0`. The caller's transaction stays open. This path does not send `COMMIT` and does not write a source row.

## Failure

An extended-query `ErrorResponse`, followed by `Sync` and `ReadyForQuery` status `E` with no pending ready count and no stream fault, clears the lease. The caller rolls the transaction back. The step does not return a row. `consumed_rows` stays 0.

Any other shape, a decode failure, a row count other than the one reserved row, or cancellation of `pull` poisons the connection. Ordinary queries and `ping` then fail. Dropping the transaction does not write and does not panic. Poison is not proof that the backend process has exited. Release of the backend within five seconds is a separate requirement.

Notices and other asynchronous messages stay on the bounded `recv` path.

## Unchanged executor

`run` still binds the unnamed portal, sends `Execute` with limit 0, closes that unnamed portal, and sends `Sync`.

## Still open

Full reader, production catalog probe, fact mapping, Arrow, source-wide budget, backend heap, the five-second backend teardown bound, R1, and release are not delivered here.
