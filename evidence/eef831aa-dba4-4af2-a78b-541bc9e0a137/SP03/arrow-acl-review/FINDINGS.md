# Findings

## 1. Post-open table REVOKE is `select_not_granted` before the scan

The a134 failure was a committed `REVOKE SELECT` on the next table, followed by `SAVEPOINT oo_stream_lock` and a scan `42501` mapped to `insufficient_privilege`.

On `dd5bf441`, the same fixture (`acl.acl_a` then `acl.acl_b`, revoke after `acl_a` `TableEof`) returns `Privilege` / `select_not_granted` from the guard and from `finish`. Log lines 111-120 show the repeatable-read `has_column_privilege` probe, then `CURRENT_CATALOG`, then two `ROLLBACK`s. No savepoint and no scan of `acl_b`. Pids 145 and 146 are gone before the next test starts.

`recheck` still calls `check_relation` and `has_column_privilege` on the repeatable-read session (`snapshot.rs` 1404-1426, `sql.rs` `SNAPSHOT_ATTRIBUTES`). Those calls did not fail the pull. `guard_still_current` did, and `stream.rs` 509-520 still places that guard before the savepoint.

## 2. The current-catalog expression

`CURRENT_CATALOG` keeps one row per physical column and appends one boolean. For a non-null `attnum` the boolean is true when any of these hold for `CURRENT_USER`:

- `pg_roles.rolsuper` on that user
- `pg_has_role(..., 'pg_read_all_data', 'USAGE')`
- `aclexplode` of `relacl`, or `acldefault('r', relowner)` when `relacl` is null, with `privilege_type = 'SELECT'` and grantee `PUBLIC` (0) or a role the user has `USAGE` on
- the same `aclexplode` test on `attacl`

`pg_has_role(..., 'USAGE')` is the inherit check. A membership without inherit does not satisfy it. `PUBLIC` is the zero grantee, short-circuited before `pg_has_role`. A null `attacl` explodes to no rows. Table owner rights come from the ACL or `acldefault`, not from a bypass, which matches a revoke of the owner's own `SELECT`.

`observe_current` compares order, relkind, oid, columns, and primary key first. A shape or key difference stays `schema_drift`. Only after that, any selected attribute whose cell is not `Some(true)` is `Privilege` / `select_not_granted`. A zero-column row has `attnum` null, `read_observed_attr` returns `None`, and the empty attribute list does not trip the check. `stream.zerocol` still streamed in `pg_stream_live` (zero batches 1 inside the payload-59 oracle). A later scan `42501` stays `insufficient_privilege` at `error.rs` 193.

The guard runs on the demand that opens a table. A row already decoded for the current table is still delivered from the pending tail in `stream.rs` before that demand. The open transaction and generation are unchanged.

## 3. Inherited grant, the representative that is not the fixture role

Direct `GRANT SELECT` to `oo_a131_reader` is what `acl_a` already exercises. The added representative grants `SELECT` only to `colg_parent` and grants that role to the reader with inherit.

Oracle, as `oo_a131_reader`: `has_table=true`, `table_acl=true`, `column_acl=false`, expression `true`, `relacl={postgres=arwdDxtm/postgres,colg_parent=r/postgres}`, `attacl` empty. The stream emitted Arrow int32 3 and `finish` returned `SnapshotFinished`. Both backends closed. Reader writes in that audit were 0.

## 4. Column ACL is real in the expression, and open still requires table SELECT

A separate probe, `pg/expression-probe.txt`, granted only `SELECT (id)` on `colg.expr_col`. As the reader the case returned `true` (`has_table=false`, `has_column=true`, table arm `false`, column arm `true`).

`SnapshotOwner::open` still rejects that grant. `check_relation` (`snapshot.rs` 1187-1188) requires `has_table_privilege` from `SNAPSHOT_RELATION`. That function and that SQL are not in the `dd5bf441` hunk. The first review binary failed there (`attempt1`, panic `select_not_granted` at open) before any batch. No row was returned.

On PostgreSQL 17.10, `REVOKE SELECT ON TABLE` also clears `attacl`. A probe sequence of column grant, table grant, then table revoke left `has_column_privilege` false and `attacl` empty. There is no post-open state where table `SELECT` is gone and the explicit column grant remains. The column arm matters for a column-only ACL the expression itself accepts. This product's unchanged open gate does not. That is fail-closed, and it is not the a134 hole. The a134 hole was a scan after a table revoke. That scan is gone.

## 5. Author comment

The committed `snapshot.rs` comments on `ObservedAttr.select` and above the privilege check do not change behavior. Blob `87c4b403` from the author note is not in this repository. This review verified the committed source by archive blob, smoke, and the runs above. It does not certify the author's evidence hashes.

## 6. Bridge, retained

`types/mod.rs` is byte-identical to `75b993d0`. The coordinator already admitted that crate-private re-export as review input. The original landing inside the product commit remains a process violation. This review does not reopen it as an R2-S functional failure.
