# Findings

## 1. REQUEST_CHANGES — post-open REVOKE misses `select_not_granted`

Severity: the revoked table is not returned, and `finish` does not return `SnapshotFinished`. The failure fact is the generic SQLSTATE map, not the privilege fact the recheck is written to return.

Observed on the decisive run, source `rev_acl`, generation 1, tables `acl_a` then `acl_b`:

- `acl_a` reached `TableEof`.
- Admin, application `oo-rev-admin`, ran `REVOKE SELECT ON rev.acl_b FROM oo_a131_reader` (postgresql.log line 2708).
- RR reader ran `SNAPSHOT_RELATION` and `SNAPSHOT_ATTRIBUTES` for `acl_b` (lines 2710-2713), including `has_column_privilege(..., 'SELECT')`.
- READ COMMITTED guard ran `CURRENT_CATALOG` (line 2714).
- Reader then ran `SAVEPOINT oo_stream_lock` (line 2716).
- Next server line: `ERROR: permission denied for table acl_b` / `STATEMENT: SELECT id FROM rev.acl_b` (lines 2717-2718).
- Then `ROLLBACK TO SAVEPOINT oo_stream_lock` and `RELEASE SAVEPOINT oo_stream_lock` (lines 2719-2720).
- Overlay outcome: `acl_guard kind=Privilege fact=Some("insufficient_privilege")`. `acl_finish` is the same fact. No `acl_b` batch.

Code:

- `crates/oo-connectors/src/sql.rs` lines 387-394. `SNAPSHOT_ATTRIBUTES` reads `has_column_privilege`. That function is STABLE, so the repeatable-read snapshot still sees the grant from open.
- `crates/oo-connectors/src/snapshot.rs` lines 1424-1426. `recheck` returns `privilege` / `select_not_granted` only when `attr.select != Some(true)`. The probe result was true, so this branch did not run.
- `crates/oo-connectors/src/snapshot.rs` lines 1967-1984. `observed_attrs_match` compares column shape only. The current-catalog guard cannot report a revoke.
- `crates/oo-connectors/src/stream.rs` lines 509-520. Order is data recheck, guard, savepoint, named portal. The portal is what PostgreSQL rejected.
- `crates/oo-connectors/src/error.rs` line 193. SQLSTATE `42501` becomes `Privilege` / `insufficient_privilege`.

Required change: a revoke committed after open and before the next table must fail the pre-scan recheck as `Privilege` / `select_not_granted`, before `SAVEPOINT oo_stream_lock` and before the scan statement. The current-catalog guard needs a privilege comparison, or the data recheck needs a privilege read that sees committed ACL changes. Mapping `42501` from the scan is the backstop observed here. It is not the recheck.

Reproducer is `overlay/pg_stream_review.rs`, section `acl_guard`. The fixture is `pg/init.sql` tables `rev.acl_a` and `rev.acl_b`.

## 2. Schema guard held

After `rev.schema_a` `TableEof`, `ALTER TABLE rev.schema_b ADD COLUMN extra int4` (log line 2604). The following demand ran the RR attribute probe and then the READ COMMITTED guard catalog query (lines 2606-2611) and rolled the transaction back (line 2612). There is no `SELECT` of `rev.schema_b`. Outcome fact `schema_drift`. `finish` returned that error.

## 3. Overlay `portal_order_bad` is a parser miss

The decisive review outcome includes `portal_order_bad=true` and `reader_writes=0`. An offline recount of `linux/functional/postgresql.log` found 1397 reader statements and 0 reader DML, DDL, or COMMIT. Writer statements: 8. Admin statements: 241.

The flag is set for two log shapes the overlay parser does not model:

- Budget wire failures (`max_value`, `max_row`) log `SAVEPOINT oo_stream_lock`, the scan `SELECT`, then a full transaction `ROLLBACK`, with no `ROLLBACK TO`. The savepoint disappears with the transaction. The parser still thinks a savepoint is open, so the next `SAVEPOINT` looks nested.
- The ACL failure logs the scan as `STATEMENT:` on the `ERROR` line, which the parser skips, so the following `ROLLBACK TO` looks like it had no `SELECT`.

Success-path portal order in the review window is `SAVEPOINT`, portal `SELECT` / `execute fetch`, `ROLLBACK TO SAVEPOINT oo_stream_lock`, `RELEASE SAVEPOINT oo_stream_lock`. Parent example: log lines 2154-2166. The fetch lines resume only after the idle wait. That is the close-before-release evidence.

## 4. Bridge hunk, retained, not a new defect

`types/mod.rs` in `75b993d0`:

```
-pub(crate) use read::guard_plans;
+pub(crate) use read::{compose_presence_scan, compose_scan, guard_plans, require_binary};
```

`decode_payload` was already `pub(crate)`. No public mapping or canonical API change. The coordinator admitted this re-export as review input. The original landing inside the product commit remains a process violation. This review does not reopen it as an R2-S functional failure.

## 5. TLS tail is outside this tree

Worktree `buffered.rs` blob `3ba2b87507096bab77464eaab7797712e4bf02cc` and untracked `tests/pg_wire_tls_tail.rs` belong to a133. They were not compiled. `init_mut` growth stays an external integration gate. This review does not approve the bounded driver integration.
