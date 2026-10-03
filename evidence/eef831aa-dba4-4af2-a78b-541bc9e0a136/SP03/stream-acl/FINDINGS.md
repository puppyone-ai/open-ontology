# Findings — a136 current ACL

## Defect

After open, a committed `REVOKE SELECT` on the next selected table was still followed by `SAVEPOINT oo_stream_lock` and `SELECT` of that table. PostgreSQL returned `42501`. The pull mapped that to `Privilege` / `insufficient_privilege`. `finish` carried the same fact. No row of the revoked table was emitted.

The repeatable-read recheck calls `has_column_privilege`. On the open guard session the same function, already prepared, kept returning true after the revoke. A direct check on PostgreSQL 17.10 prepared the catalog statement, revoked `SELECT` on `acl.acl_b` from another session, and executed it again: `has_column_privilege` stayed true while `relacl` on that row no longer contained `oo_a131_reader=r`. `aclexplode` of that `relacl` returned false for `acl_b` and stayed true for `acl_a`.

## Change

`CURRENT_CATALOG` still returns one row per selected column, with the same indexes 0 through 14. Index 15 is now whether `SELECT` is in that row's table ACL (`relacl`, or `acldefault` when the ACL is null) or column ACL (`attacl`). The expression counts `PUBLIC` (grantee 0), roles `CURRENT_USER` can use, a superuser, and `pg_read_all_data`.

`observe_current` compares shape and primary key first. If either differs, the result stays `schema_drift`. If a selected column's privilege cell is not true, the result is `Privilege` / `select_not_granted`. A table with no selected column still has an empty attribute list, so this check does not fire. `42501` from a later scan stays `insufficient_privilege`.

`stream.rs` already runs `guard_still_current` before `SAVEPOINT oo_stream_lock` and the portal. A held batch of the table already being delivered is still delivered. The next table is the one the guard refuses. Row reservation and `LIMIT` are unchanged. The data transaction and generation are unchanged.

## Decisive log

`linux/functional/postgresql.log`, ACL window before `oo-stream-admin`. Admin `REVOKE SELECT ON acl.acl_b FROM oo_a131_reader` is a separate session. After it, the reader log has the current-catalog statement and no second savepoint, no `SELECT id FROM acl.acl_b`, and no `permission denied`. Outcome: guard and finish `select_not_granted`, `reader_writes=0`, `backends_gone=2`, generation 1, `acl_a` Arrow `id` 7.

`pg_stream_live` and `pg_stream_bounds` exited 0 on the same log and the same cut. Live Arrow payload stayed 59 bytes against the pre-writer oracle. Late writer rows stayed out of that snapshot.
