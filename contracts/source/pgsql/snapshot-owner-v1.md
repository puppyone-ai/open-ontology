# PostgreSQL snapshot owner `oo.source.pgsql.snapshot/v1`

R1 only. R2, R3, and R4 are unsigned. This is not the bounded reader. `scan`, `bounded_read`, `cdc`, `type_conversion` on the catalog protocol, and `registry_api` stay unsupported. Catalog `type_support.converted` stays false.

The owner reuses `map_column`, `guard_plans`, `decode_payload`, and `assemble_batch` from `oo.source.pgsql.types/v1`. `decode_payload` and `guard_plans` are re-exported crate-visible from `types/mod.rs` so the owner can call them. That re-export does not change mapping or decode semantics. `collect_catalog` and `collect_access` keep their result shape. Their reads admit a row bound before SQL.

## What the caller passes

`SnapshotRequest` carries an admitted `PgSourceConfig`, an `AdmittedCatalog`, `config_identity`, `type_policy_identity`, a `PgTypePolicy`, a `generation`, a `deadline`, and the expected principal, database, `pg_is_in_recovery()` value, and `pg_postmaster_start_time()::text`.

`config_identity` is SHA-256 of `oo-pg-config-identity/v1` over source id, host, port, database, username, secret reference, budgets, and selected names. The password and the CA PEM are not in that preimage. `type_policy_identity` is SHA-256 of the policy names. `snapshot_input_digest` is SHA-256 of the two identities, the raw 32-byte `schema_hash`, and the generation as little-endian `u64`.

`AdmittedCatalog` names the selected tables and the column shape (`attnum`, name, type oid, typmod, `attnotnull`) plus primary-key names in key order. The table set must match `TableSelection`.

## Open

`SnapshotOwner::open` refuses generation `0`, a deadline that has already passed, and an identity mismatch before it reserves the generation. Those refusals do not consume the generation.

After that, the generation is reserved for the process. A later open of the same source id and generation returns `generation_reused`, including when the first open failed after reservation. The reserve set is process-local. It is not a cluster lease.

Connect uses the existing `PgConnector` path: `verify-full`, caller CA only, `application_name=oo-pg-source`, no startup options, tested version `17.10`, and `session_user = current_user`. `session_user_mismatch`, `startup_options`, TLS failure, and an untested version stop the open. The owner then checks the connected principal and database against the request.

The owner calls `BEGIN` once: `START TRANSACTION ISOLATION LEVEL REPEATABLE READ, READ ONLY`. A second `begin` on that session returns `nested_transaction`. `PgSession::catalog` and `PgSession::read_typed` are not used to create this transaction. `collect_catalog` runs inside it.

Catalog and access statements reserve their own row bound before the round trip and bind that same bound as a static `LIMIT`. Settlement returns unused allowance in the same one-second window. A failed or timed-out read keeps the reservation. A refund does not cross into a later window. An exact natural bound may be full. A result that fills a non-natural limit is `catalog_truncated`. Keyword admission reserves the embedded reserved-word count plus one.

Inside the transaction the owner reads `pg_is_in_recovery()`, `pg_postmaster_start_time()::text`, `current_database()`, `session_user`, and `current_user`. A recovery or postmaster mismatch is `read_node`. It then checks each selected relation: ordinary table (`relkind = r`), RLS off, `SELECT` granted, and `INSERT` / `UPDATE` / `DELETE` / `TRUNCATE` all false. `NULL` is `privilege_unknown`. A true write privilege is `write_capable`. Missing `SELECT` is `select_not_granted`. The attribute set must match the admitted shape or the open returns `catalog_identity`. The attribute set and primary key are compared with the admitted shape inside this transaction. A mismatch is `catalog_identity`. A drift between that captured shape and a later current-catalog observation is `schema_drift`. The current observation is a second session in `READ COMMITTED READ ONLY`. It is not a second read of `pg_attribute` inside this repeatable-read transaction. The data connection remains the one repeatable-read read-only transaction. The second session is reserved against the source cap of 4 before either socket is opened.

R1 primary keys must be one to eight columns of int2, int4, int8, bool, text, varchar, or bpchar. Other key shapes return `probe_key` and do not return an owner.

The backend key is `pg_backend_pid()` and `backend_start::text`, and `xact_start` must be set. The same connection cannot see itself as `idle`. An admin session observes `idle in transaction` between owner statements.

The deadline is checked before connect, before `BEGIN`, before and after catalog collection, and before every probe statement. It is not polled inside `collect_catalog`. A deadline that expires during a statement already sent is reported when that statement returns. The server `statement_timeout` still bounds the statement. That check is not in-flight cancellation.

On any failure after connect, the owner rolls back and closes. If that cleanup fails, the error is the cleanup error. The owner is not returned.

## Probe

`read_probe` borrows the open owner. It re-reads node identity, relation oid, attributes, and privileges, then runs one statement:

```text
SELECT <quoted columns> FROM <quoted table> WHERE <pk> = $1 AND ... LIMIT $n
```

Identifiers come from `quote_ident` on the bound catalog and must pass `valid_quoted_ident`. Values are bound. The limit parameter is `2`. Zero rows returns an empty `TypedBatch`. Two rows is `probe_key`. This is not `fetch_all` of the table.

Decoded rows use the existing binary decoder and `assemble_batch`. The batch protocol stays `oo.source.pgsql.types/v1`. The batch `run_generation` and `input_digest` are this cut. The public API does not return the connection or accept caller SQL.

A privilege or shape mismatch on the recheck stops the probe. `has_*_privilege` is the server's answer for that statement. This contract does not say MVCC freezes ACL checks. A denial is `select_not_granted` or SQLSTATE `42501`, and `finish` is not `SnapshotFinished`. A difference of relation oid, relkind, attnum, name, type oid, typmod, nullability, or primary-key attnum and name order, between the captured shape and the current committed catalog, is `schema_drift`. The owner is poisoned. `finish` is not `SnapshotFinished`. The same compare runs before the probe statement, after it, and on finish, including when no probe ran. A principal or database change is `identity_drift`. A recovery or postmaster change is `read_node`.

Each current-catalog statement observes commits that landed before that statement's snapshot. The post-probe check and finish detect DDL that committed before those observations. The owner does not claim that DDL cannot commit after the last observation. An `ALTER` that has not committed and blocks the probe is `lock_timeout` (SQLSTATE `55P03`) or `statement_timeout`. That is not `schema_drift`. The owner is poisoned, and finish is not `SnapshotFinished`.

Any probe error poisons the owner.

## Finish, abort, drop

`finish` reads the current catalog before rollback when the deadline has not yet passed and the owner is not already poisoned. A mismatch is `schema_drift`. `SnapshotFinished` still requires accepted rollback and close of every session this owner opened, an empty poison, and a deadline that has not passed. Cap refusal before connect does not consume the generation. Failure after the generation is reserved does. Cleanup failure returns that failure and not `SnapshotFinished`.

`abort` uses the same cleanup. Confirmed cleanup returns `SnapshotAborted` even if the owner was poisoned. That type is not `SnapshotFinished`.

`Drop` does not return either type. If a Tokio runtime is current, drop spawns rollback and close. Otherwise it drops the socket. Neither path is a confirmed rollback, and neither path is the R3 in-flight cancel proof.

After a confirmed close, an admin poll of `pg_stat_activity` for that `(pid, backend_start::text)` should find no row within `CLEANUP_OBSERVATION_BOUND_MS` (5000). The row must not remain `idle in transaction`. The product connection cannot observe its own `idle` state: a confirmation query on that backend is `active`, and a read-only transaction may have a virtual xid, so `txid_current_if_assigned()` is not the proof.

There is no commit, no source DML, and no source DDL. The data transaction may `SAVEPOINT oo_probe_lock`, `ROLLBACK TO SAVEPOINT oo_probe_lock`, and then `RELEASE SAVEPOINT oo_probe_lock` around a probe read. `RELEASE` alone is not the lock drop. The client does not send `COMMIT`. There is no second data transaction and no reconnect on the owner. The generation cannot be appended to.

## Later stream, not this slice

A later `read_stream` would borrow this owner and this transaction, return a stream of `TypedBatch`, page with a server-side cursor or portal, call `decode_payload` per row, and refuse `fetch_all` of the table. Dropping the stream would not finish the owner. R3 is the in-flight cancellation proof. R1 does not implement that method.

## Errors

Facts used by this owner include `generation_invalid`, `generation_reused`, `config_identity`, `type_policy_identity`, `deadline`, `session_user_mismatch`, `startup_options`, `principal`, `database`, `read_node`, `catalog_identity`, `nested_transaction`, `snapshot_transaction`, `write_capable`, `privilege_unknown`, `select_not_granted`, `rls_unsupported`, `relkind`, `schema_drift`, `identity_drift`, `probe_key`, `snapshot_incomplete`, `cleanup_failed`, `max_connections`, and `max_rows_per_sec`, plus the existing version, TLS, and connection facts. `max_connections` and `max_rows_per_sec` use the existing budget kind. `Display` stays kind, static fact, and SQLSTATE. Logs and batches do not carry the password, the CA PEM, or driver text.
