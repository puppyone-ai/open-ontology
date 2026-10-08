# Sequence fence addendum

Session `eef831aa-dba4-4af2-a78b-541bc9e0a377`. This file supersedes only the sequence-lock sentence in `CONTRACT-FREEZE.md`. It was written after the PostgreSQL 17.10 measurements in `seq-lock-probe/` and before the product edit that uses this fence. It does not accept E51, SP14-W04, AT-OPS-06, PITR, pgBackRest, a production SLO, or a parent goal.

`LOCK TABLE` on a sequence still fails with `42809`. That refusal is not a delivery. A cut that can never hold a sequence fence does not close this repair.

## What PostgreSQL 17.10 does

`AlterSequence` takes `ShareRowExclusiveLock` (`sequence.c`, `RangeVarGetRelidExtended`). That mode conflicts with `nextval` and `setval`, which take `RowExclusiveLock` in `lock_and_open_sequence` before the backend-local cache is dispensed. It does not conflict with `pg_dump`'s `ACCESS SHARE`. `LOCK TABLE` cannot ask for this mode: `RangeVarCallbackForLockTable` allows only relkind `r`, `p`, and `v`.

`init_params` rewrites the sequence tuple for every option except `OWNED BY`. A same-value `CACHE 1` and a same-value `INCREMENT BY 1` were measured: `last_value` stayed, `log_cnt` went to 0, and `relfilenode` changed. Those statements are not the fence. `RESTART`, `nextval`, and `setval` change values and are not the fence.

`ALTER SEQUENCE ... OWNED BY NONE` on an unowned sequence, and `ALTER SEQUENCE ... OWNED BY` restating the current column, were measured on this server. `last_value`, `is_called`, `log_cnt`, and `relfilenode` did not change. `pg_dump --snapshot` of that open transaction wrote `setval('shop.cut_seq', 41, false)` and kept `ALTER SEQUENCE shop.owned_seq OWNED BY shop.entry.id`.

Concurrent `nextval` and `setval` against the held `ShareRowExclusiveLock` failed with lock timeout (`55P03`). A second `nextval` in the same backend, after that backend had cached values from `CACHE 10`, also failed with `55P03` and did not move the on-disk row (`during|10|t|32`). The cache does not skip the lock. Values already fetched onto disk before the fence stay in the dump; that is the on-disk state the fence froze. An in-flight `nextval` that already holds `RowExclusiveLock` makes `ALTER SEQUENCE` wait (`55P03` with `lock_timeout`); the fence is then not held.

`nextval` in a read-only transaction fails for that transaction only. It does not fence other sessions.

## Operator

`ALTER SEQUENCE` requires ownership. `USAGE` and `UPDATE` do not. `LOCK TABLE` on a table is allowed by `MAINTAIN`, `UPDATE`, `DELETE`, or `TRUNCATE` (`LockTableAclCheck`). The registered roles are `oo_a377_owner` (`NOLOGIN NOSUPERUSER`) and `oo_a377_operator` (`NOLOGIN NOSUPERUSER INHERIT`). `oo_a377_owner` owns the user tables, views, and sequences. `oo_a377_operator` inherits that owner. The cut session is `SET ROLE oo_a377_operator` before the table locks and the sequence statements. Measured: `current_user` was `oo_a377_operator`, `rolsuper` was false, and both `LOCK TABLE` and `ALTER SEQUENCE ... OWNED BY` succeeded. Granting the superuser role to the operator is refused by PostgreSQL's membership check and is not used. The operator is not a connection to an enterprise source.

## Cut rule

On each source database, after `BEGIN ISOLATION LEVEL REPEATABLE READ` and `SET ROLE oo_a377_operator`:

1. `LOCK TABLE` of user tables (`relkind` `r` and `p`) `IN SHARE MODE`, held through both dumps.
2. For each user sequence, restate ownership: `OWNED BY NONE` when `pg_depend` has no owner column, otherwise `OWNED BY` the current table and column. Read `last_value`, `is_called`, and `log_cnt` before and after. A change fails that sequence's fence. No `RESTART`, `CACHE`, `INCREMENT`, `nextval`, or `setval`.
3. `pg_export_snapshot()`, then the catalog read, then both `pg_dump --snapshot` calls, while the transaction is still open.

`sequence_fence.held` is true only when every sequence returned sqlstate `00000`, the state did not move, and `ShareRowExclusiveLock` is still granted on each sequence after both dumps. The manifest fields are `lock_mode` `ShareRowExclusiveLock` and `method` `alter_sequence_owned_by`. A database with no user sequence has nothing to fence. The two snapshot ids must still differ. The dump remains a full custom dump. No relation is removed.

The harness, not the product, calls `nextval` and `setval` on the preexisting sequences while that lock is visible. Both must fail with `55P03`, and both sequences must stay `41|f` through the dump and the fresh restore. The amount update must still fail with `55P03` and the dump must keep `1234567890.1234`. Objects created after the snapshot must stay out of the dump. A proven cut may restore. `pitr_ready`, `parent_acceptance`, `production_ops`, `production_slo_certified`, `all_state_certified`, and `offset_min_used` stay false. `w04_gate` stays open. There is no `development_unit` readiness field.
