# Gateway SELECT executor

Contract `oo.gateway.query-executor/v1`. Library `oo_gateway::query_executor`.

This slice runs one guarded `SELECT` for a trusted runtime on one dedicated TLS session. It calls `guard_select` on the caller's original SQL and executes `GuardedPlan.executed_sql`. The request has no executed-SQL field. The caller cannot hand the executor a string that skipped the guard.

SP06-W04, SP06-W05, and SP06-W06 stay open. This contract does not accept those parent items, the gateway, or a release.

## Trusted runtime only

`DedicatedSession::execute_trusted_select` is the only entry. `UNTRUSTED_ADMISSION` is `Blocked`. The parser worker exists, but its kernel sandbox is unsupported and this crate has no authentication gate, so untrusted admission stays blocked. There is no public function that accepts an untrusted statement. The a179 review's boundary still holds for the guard: this module does not publish `guard_select` as a public read API. It consumes the guard from inside the trusted runtime.

The API takes the caller's current `PolicyCatalog` and `PolicyIdentity` as trusted adapter input, plus the exact `NOLOGIN` role string. It does not accept an HTTP request, a token, a CLI argument, or an authentication claim. It does not assume row filters or claims. A row filter remains refused by the column-ACL install.

## Session

One `tokio_postgres` client, connected by the platform with TLS. This slice has no pool and does not add a pool type. `DedicatedSession::bind` reads `pg_backend_pid()`, the timezone-stable `backend_start` (`to_char(... AT TIME ZONE 'UTC', 'YYYYMMDDHH24MISSUS')`), and `pg_stat_ssl.ssl` before any user statement and before any cancel. `ssl` must be true. The platform login has to be able to read its own `pg_stat_activity` and `pg_stat_ssl` rows.

The role string is read after the session-open check and after the row, byte, and wall limits, and before `guard_select` and before `AuditAdmission::admit`.

A role binds only when it is `oo_r_` plus a lowercase slug of 1..=58 characters and it equals `identity.role`. That first valid role is stored on the dedicated session before the guard runs. An invalid role, or a role that does not equal `identity.role`, returns `RoleParameter` and does not set or clear the stored role. A limit error returns before this check and does not bind.

A later request whose valid role string differs from the stored role returns `RoleReuse` before the guard and before the audit hook. That is also the result when the first request was refused by the guard or by `admit`. The same valid role may be submitted again. The session does not run one connection as two roles.

The `pg_roles` flag check still runs later, inside the `REPEATABLE READ READ ONLY` transaction, before the user statement. The read is parameterized and expects one row. Login, superuser, createdb, createrole, inherit, bypassrls, and replication must all be false. A missing row or any true flag is `RoleParameter` and closes the socket. The next call on that session is `SessionClosed`.

The user statement runs in one `REPEATABLE READ READ ONLY` transaction. Before that statement the executor sets, once:

- `SET LOCAL search_path TO ''`
- `SET LOCAL ROLE` to the quoted NOLOGIN role
- `SET LOCAL statement_timeout` to the remaining client budget minus about 250 ms, floored at 1 ms

It then reads `transaction_isolation`, `transaction_read_only`, `current_user`, `search_path`, and `pg_settings.setting` for `statement_timeout`. A mismatch is a protocol failure with a static step label. Empty `search_path` is `""` or an empty string.

PostgreSQL restarts `statement_timeout` on each portal `Execute`. This executor does not refresh a fresh 30 s per page. The whole-query bound is one client `Instant` taken at `execute_trusted_select` entry. It covers admission, blocking, network, and drain. The server timer is not claimed to be a single 30 s across portal pages.

The accepted limits are 30 s, 2 000 rows, and 256 KiB. The wall clock must be a whole millisecond count from 1 through 30 000. `max_rows + 1` may be fetched. A row that would pass `max_rows`, or whose canonical bytes would pass `max_result_bytes`, is not appended, and the summary is truncated. Canonical sizes are 0 for null, 4 for `true`, 5 for `false`, and the decimal or raw text length otherwise.

User SQL is a prepared statement, bound with an empty parameter list, fetched with `query_portal` page size 1. Page size 0 is forbidden because it asks the server for every row. The stream is an async channel of capacity 1. The producer does not collect the full result. Platform `BEGIN` and `SET LOCAL` may use the transaction builder and `batch_execute`. User SQL does not use `simple_query`.

## Types

The prepared statement's result OIDs are checked before the portal is executed. The initial set is bool (16), int8 (20), int2 (21), int4 (23), and text (25). Null of those types is `Cell::Null`. Integers are decoded with the binary typed protocol and rendered with `Display`, including `i16::MIN` and `i64::MIN`. `varchar`, `numeric`, `unknown` (705), and every other OID are `TypeRefused`. There is no lossy string conversion and no full type certificate.

## Cancel, failure, reuse

`CancelHandle` stores the generation captured when the query became cancellable. `cancel` reserves that generation when it is still the accepting generation, then observes `pg_stat_activity` from a different session. It checks the generation, acceptance flag, pid, and `backend_start`, and only then calls `cancel_query`. There is no await between that check and the cancel call. The token's process id does not change for the life of the connection, so the generation is the fence.

Reserving the generation fences the session. `execute_trusted_select` returns `SessionBusy` while the producer is running or while that reservation stands. A successful query does not lend the connection until the reservation clears. If the cancel request was written, the connection is not lent. If the reserved transport exceeds the cleanup deadline or returns a transport error, the session is fail-closed: it stops accepting cancels, drops the socket, and the next call is `SessionClosed` once the producer has exited. A later packet for that pid is not delivered to another generation on this connection. A handle that does not match the accepting generation does not reserve and does not send, even when a later observation would still name the same pid. A second cancel for a generation that is already reserved returns `NotSent`. An observe timeout on a handle that did not reserve returns `Timeout` and does not fail-close the session.

The future that reserved the generation owns that reservation. `CancelHandle` is `Clone` and has no `Drop`, so dropping a handle does not release it. If that future is dropped before `cancel_query` is entered, it releases only its own generation and leaves the session open. If it is dropped after the transport is entered, it fail-closes the session on the same thread: the destructor does not await, the owning reservation is cleared, and waiters are notified. A clone that did not reserve returns `NotSent` and does not release the owner's reservation. When the producer leaves without reuse, or the shared remainder is already zero and a reservation is still set, that leftover reservation fail-closes the session. Once the producer has exited, the next `execute_trusted_select` on that closed session is `SessionClosed`.

Dropping `SelectStream` sets the producer shutdown flag. That path does not abort the producer, and an in-flight database future is not dropped to stand in for cancellation. `DedicatedSession` drop and `close` abort the local producer and driver so a cancel future owned by the session is not left running. That abort is local teardown. It is not `cancel_query`, and it does not prove the PostgreSQL backend stopped.

If shutdown is already set before a database call is polled, that call is not started and `cancel_query` is not sent for it. If shutdown arrives while a call is being polled, the producer checks the stored generation, acceptance flag, pid, and `backend_start`, and only then calls `cancel_query`. There is no await between that check and the call, and this path does not open a second session. A mismatch does not call `cancel_query`.

Reachable cleanup stays inside one 750 ms deadline taken at the first cleanup action. Observe, cancel connect, TLS, the cancel send, the pinned server response, and `ROLLBACK` all spend that same budget. A phase that starts later uses only the time still left. It does not receive a fresh 750 ms. The deadline is not a per-page query budget. It does not refresh the whole-query `Instant` or `statement_timeout`. PostgreSQL still restarts `statement_timeout` on each portal `Execute`; the client budget stays the one `Instant` from `execute_trusted_select`.

When the server is reachable, a consumed response leaves the socket synchronized. `ROLLBACK` then uses the time still left. A rollback that returns is recorded as confirmed. The completion status is `partial`, the stop class is `Protocol`, the step is `consumer`, and SQLSTATE is absent. That classification holds when the consumed server error was `57014`.

Unreachable cleanup fails closed. If the deadline expires during cancel transport, or that transport returns an error, the cancel future is dropped, the pinned response is not given another budget, the socket is dropped, and rollback is not claimed. The status stays `partial` with stop class `Protocol` when the consumer had already shut down. The summary does not say the backend is gone. A later `pg_stat_activity` read is the only confirmation: `Ok(None)`, or the same pid with a different `backend_start`, means that row is absent. An observe error leaves the backend unconfirmed. Exceeding the cleanup deadline is an SLO miss.

A client deadline that fires while a call is in flight uses the same one budget for the cancel transport, drops that unread future, and completes as `failed` with stop class `Timeout` and SQLSTATE absent, including when the transport itself expires. Rollback is not claimed. A call that has not been polled when the client deadline is already expired does not start, and its `ROLLBACK` still uses the whole-query budget rather than a second cleanup budget. A `CancelHandle` whose `57014` arrives without consumer shutdown completes as `failed` with stop class `Canceled` and keeps SQLSTATE `57014`. Its `ROLLBACK` uses the shared cleanup deadline, or one 750 ms budget if none has started. Any other server `57014` completes as `failed` with stop class `Timeout`, keeps SQLSTATE `57014`, and its `ROLLBACK` uses the time left on the whole-query `Instant`. The message text is not stored. Other database errors expose SQLSTATE only. A `complete` hook failure does not rewrite any of these statuses.

`Transaction` drop is not a confirmed rollback. Success awaits `rollback` and re-reads pid and `backend_start` before the client may be reused. The reuse decision also requires that no cancel is reserved and that no cancel request was written for this generation. Any result other than that success drops the socket even after a confirmed rollback. The client is not returned to a pool. There is no pool. A closed session is not reused, including across roles.

## Audit

`AuditAdmission::admit` must succeed before `BEGIN`. The live fixture writes one row with a second client (`Client::execute`, autocommit) and is not a production audit WAL. Guard refusal does not call the hook. Hook failure returns `AuditUnavailable` and does not start the query. The role is already bound when that happens. `complete` runs after the summary is stored. A stream the consumer drops completes as `partial`, never `succeeded`. A cancel handle or a deadline completes as `failed`. A `complete` failure does not rewrite that status into success. The summary's `end_audit_ok` flag records the hook result separately.

`QuerySummary`, `Cell`, `Admission`, and `CompletionRecord` are built by this crate. Fields are private. There is no public constructor a caller can use to forge a completed read.

## API codes

| Executor result | `ErrorCode` |
|---|---|
| `Guard` | the guard's code |
| `AuditUnavailable` | `AuditUnavailable` |
| `Timeout`, `Canceled` | `Timeout` |
| `TypeRefused` | `StatementNotAllowed` |
| `RoleParameter`, `RoleReuse`, `Limit`, `SessionClosed`, `SessionBusy` | `SchemaInvalid` |
| `Database`, `Protocol` | `Internal` |

`Display` is static text plus an OID or a SQLSTATE. It does not include SQL or a password.

## What stays open

- untrusted admission, which stays blocked because the parser worker's kernel sandbox is unsupported and the authentication gate is absent
- authentication, HTTP, tokens, and the CLI
- a production audit WAL
- row-level security, claims, and row filters
- a connection pool
- export, cell truncation, quality warnings, and `EXPLAIN` cost
- result OIDs outside bool, int2, int4, int8, text, and null
- parent SP06-W04, SP06-W05, and SP06-W06

The live fixture applies frozen migrations `0000`, `0001`, `0003`, `0004`, and `0005` itself, then `install_column_acl` for two column roles. It does not call the pipeline crate and does not edit those migrations. The portal stop check counts `pg_stat_user_functions` for one volatile plpgsql function after that query backend disconnects. PostgreSQL leaves those counters pending until the backend flushes them. Rollback alone does not publish the count.
