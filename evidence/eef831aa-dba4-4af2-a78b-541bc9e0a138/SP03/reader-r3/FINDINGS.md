# FINDINGS — a138 reader R3

## Deadline versus server caps

`SNAPSHOT_DEADLINE_DEFAULT` is 300 seconds. The caller passes the `Instant`. `PgBudget` statement and lock timeouts stay capped at 120 seconds. A 300 second run deadline does not extend `statement_timeout`. The cancel fixture used a 20 second deadline, a 60 second statement timeout, and a 120 second lock timeout. The blocked scan returned budget fact `deadline` at 19787 ms, which is the client deadline and not the 60 second statement timeout.

## Declared keys

A supported plain primary key is bool, int2, int4, int8, text, varchar, or bpchar, all NOT NULL, with no predicate and no expression. Otherwise the first catalog-order unique key with those constraints is the declared key. A uuid primary key is not a declared key, even when uuid is a lossless Arrow value. The live `uniq` table therefore plans the unique text column `code`. A table with no such key is one unkeyed full stream, `keyed` false, `resumable` false, and no fabricated resume value. PostgreSQL orders and compares the key. Rust does not sort it.

## Cancel path the server took

`pg_cancel_backend` ran on the guard session inside its `READ ONLY` transaction. The receiving pull returned protocol fact `cancelled`, and `SnapshotOwner::cancel` returned `RunCancelled`. The guard did not return `25006`. The socket-close fallback was not the path this server took. The reader log contains `pg_cancel_backend` and does not contain `pg_terminate_backend`. There is no third connection.

The same watch covers portal Parse/Bind and each Execute. A lock wait can sit in Parse before any Execute. The future stays pinned until the server answers or `CLEANUP_OBSERVATION_BOUND_MS` (5000) passes. Dropping that future is what produces `cleanup_unconfirmed`. This run did not take that branch.

## Dropped next_batch

Dropping `next_batch` while the data backend was `active` left it `active`. The outcome line is `dropped_future_backend_still_active`. Cancel after that drop released the backends. A dropped pull is not backend exit.

## Unpublished run

`SnapshotFinished` was not returned after the admin terminated the data backend. The next read failed with protocol fact `database` and SQLSTATE `57P01` (`admin_shutdown`). That SQLSTATE already maps through the existing unknown-state path. No new error constructor was added. The old output was `[1]`. After the writer inserted id 4, generation 9 read `[1, 2, 3, 4]` and then `finish` returned `SnapshotFinished`. The old rows were not appended to the new generation.

## Release

`RunCancelled` is not `SnapshotFinished`. `finish` after the cancel flag returns protocol fact `cancelled`. After each cancel or failed finish, both reader backends were gone within the 5000 ms observation bound. The product connection does not query its own idle state.

## SQL

Reader statements are SELECT, WITH, START, ROLLBACK, SAVEPOINT, or RELEASE. The live audits found no reader INSERT, UPDATE, DELETE, TRUNCATE, MERGE, COPY, CREATE, ALTER, DROP, GRANT, REVOKE, COMMIT, or OFFSET. Dynamic key SQL uses bound row comparisons and `ORDER BY` the declared columns with no `COLLATE` clause, so `attcollation` applies. The page count is an unnamed window column. An alias `n` collided with a key column (`42702`) and was removed before the keys commit.
