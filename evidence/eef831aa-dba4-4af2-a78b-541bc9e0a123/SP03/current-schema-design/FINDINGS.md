# Findings — current catalog observation

Verdict of this analysis: the implementation path is frozen in `DESIGN.md`. R1 stays unaccepted. No plan box is checked.

## F1 — same-snapshot reread cannot see the committed column

Proved by a121 attempt 6 and by PostgreSQL 17's definition of repeatable read.

Generation 41 returned `int:5 text:pebble` and `SnapshotFinished` after `ADD COLUMN extra text` had committed at attnum 6. `recheck` reads `SNAPSHOT_ATTRIBUTES` on the data session. That statement sees rows committed before the transaction's first query. The new `pg_attribute` row is later, so `attributes_match` succeeds.

<https://www.postgresql.org/docs/17/sql-set-transaction.html>

Read Committed, including successive statements of one transaction, sees commits that landed before each statement.

<https://www.postgresql.org/docs/17/transaction-iso.html>

The internal catalog snapshot (`GetCatalogSnapshot` in `src/backend/utils/time/snapmgr.c`) is not a result the client `SELECT` returns. `pg_export_snapshot` exports the transaction snapshot. Pointing the guard at it would keep the stale view.

Hypothesis, not a second measurement: the client scan uses `GetTransactionSnapshot` / `CurrentSnapshot`. a121 already shows the client scan is stale. The coder does not need to call server internals.

## F2 — a second session is required, and it has to be counted

The data session cannot take a newer snapshot without a new transaction or a new isolation level. Both are forbidden by the owner contract and by the a121 receipt. Subtransactions share the repeatable-read snapshot; they do not refresh it. The documented read-only disallow list does not name `SAVEPOINT`. It does name `ALTER` and `COMMIT`.

The guard is one extra `PgSession` from `connect_reserved`, same CA, verify-full, principal, database, postmaster, tested 17.10, and `SESSION_GUARDS`. Its transaction is `START TRANSACTION ISOLATION LEVEL READ COMMITTED, READ ONLY`. Observations are later statements. Release is `ROLLBACK` and close.

`try_reserve(source_id, 2)` runs before generation reserve and before either socket. The cap stays 4. The row rate stays 50,000 per second, fail immediately, no waiter queue. One owner uses 2 of 4 until both closes return. That is the cap interaction. It is not a fifth slot and not an uncounted transient.

`PgConnector::connect` today reserves nothing. `REQ-SRC-03` already says every startup connection counts. Public `connect` reserves 1 so the guard cannot be the only counted path. The ledger is process-local, as the generation set already is. It is not a cluster lease. The full-load window defaults to null and is not this slice.

Catalog collection inside the owner is charged after it returns, bounded by the existing row and byte caps. One response can exceed the rate and then fail the open. Guard and probe charges happen before those statements.

## F3 — locks hide the failure a121 measured

`ADD COLUMN` takes `ACCESS EXCLUSIVE`. `SELECT` takes `ACCESS SHARE`. They conflict. Locks last until transaction end, except locks taken after a savepoint and released by `ROLLBACK TO SAVEPOINT`.

<https://www.postgresql.org/docs/17/sql-altertable.html>
<https://www.postgresql.org/docs/17/explicit-locking.html>
<https://git.postgresql.org/pg/commitdiff/b9d94670c1d9b9a6604bb5cd80f77e93aee4e776>

a121 committed `ADD COLUMN` before the user-table probe, so the open path does not hold `ACCESS SHARE` on `drift.item`. A probe `SELECT` would hold it until rollback. A finish-time observe that runs while that lock is held cannot see a committed `ADD COLUMN` on that table, because the `ALTER` cannot commit.

The probe statement is wrapped in `SAVEPOINT oo_probe_lock` and `ROLLBACK TO SAVEPOINT oo_probe_lock` after the rows are in memory. The snapshot stays the one taken at `capture`. `RELEASE SAVEPOINT` is the wrong statement: it keeps the lock.

Hypothesis to execute on 17.10: those two statements succeed in this read-only transaction, the old row token is unchanged, and `ADD COLUMN` after the probe commits before `finish`. If either part fails, stop. Do not replace the check with a lock that makes `SnapshotFinished` true because DDL never committed.

SQLSTATE `55P03` stays `lock_timeout`. It is an uncommitted waiter, not `schema_drift`.

## F4 — observation points and the shape compare

Current compare runs after `capture`, before each probe `SELECT`, after each probe `SELECT`, and at `finish` when no probe ran. Every selected table is compared, not only the probed name.

Drift is relation row count other than 1, oid, relkind other than `r`, attnum, name, type oid, typmod, not-null, or primary-key attnum and name order. Drop plus create of the same name is an oid change. Attnum of a new column is whatever the admin `SELECT` records. Privilege changes stay on the data session's `has_*_privilege` recheck.

A batch already returned is the old cut. `SnapshotFinished` is the success signal and is withheld. A probe whose post-observe sees drift does not return the batch.

## F5 — release, secrets, and the audit

Both sessions use `release_session`: rollback, then `Connection::close`. Permits return after close on that path, and on `Drop` so a panic cannot pin the cap. The product does not sleep for the 5-second poll. The test polls `pg_stat_activity` for both pids.

`Debug` and statement logging stay as they are. The ledger stores a source id and counters. Guard parameters are names and limits.

Both backends use `application_name=oo-pg-source`, so the audit cannot miss the guard by looking at one actor. The new allowed verbs are the savepoint pair. `COMMIT` and source DDL stay at zero.

## F6 — contract sentence is the stale definition

Contract line 25 defines `schema_drift` as drift from the catalog captured in the same transaction. That sentence describes the check a121 showed is blind. Line 47 says a shape or oid mismatch is `schema_drift` and does not say the second read may be the same snapshot. The prepared review, line 11, requires current schema changes to stop. The receipt says not to weaken the requirement to old catalog visibility.

The coder replaces those sentences with the text in `DESIGN.md`. `catalog_identity` remains the admitted-versus-captured compare. `schema_drift` becomes captured-versus-current.

`REQ-PL-07` is pipeline handling of a new nullable column. a121 left it outside this slice. This design does not implement it.

`identity_drift` and `nested_transaction` were not executed at runtime in a121. This design does not add a public connection or a second data `begin`, so those two facts stay untested. Role rename still does not change `session_user`.

## F7 — no execution in this session

No builder, no toolchain install, no PostgreSQL, no container. Smoke and C0–C15 are the coder's runs. This file does not claim them passed.
