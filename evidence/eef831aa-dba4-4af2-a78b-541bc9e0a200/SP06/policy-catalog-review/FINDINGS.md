# Findings

Verdict: LIMITED.

Fixed source `78a60fdeabcada0d024fc6f9a81e0e6b184260e8`. The loader, installer, guard, and dedicated session were exercised together on database `oo_a200`. The oracle is `live/oracle.txt`. Raw exits are under `linux/smoke/phases/` and `linux/proof/phases/`.

## F1. The author bound-role claim does not hold on this source

a190 `FINDINGS.md` line 39 says the executor binds the role after `guard_select` and that the live test sees an empty `bound_role()` after a guard refusal. That run used archive `3a251deacd7e86fc3fbe0855be3e1f6ec976c9d6`.

On `78a60fde`, `DedicatedSession::execute_trusted_select` records the role before the guard:

```628:642:crates/oo-gateway/src/query_executor.rs
match role_bind_decision(
    guard.bound_role.as_deref(),
    request.role,
    &request.identity.role,
) {
    RoleBind::Invalid => return Err(ExecError::RoleParameter),
    RoleBind::Reuse => return Err(ExecError::RoleReuse),
    RoleBind::Proceed => {
        if guard.bound_role.is_none() {
            guard.bound_role = Some(request.role.to_string());
        }
    }
}
let plan = guard_select(request.identity, request.catalog, request.original_sql)
```

Live counterexample, fresh session, saturated catalog, threshold 10:

- Before the statement, `bound_role()` was empty.
- `SELECT * FROM warehouse.parcel` returned `ExecError::Guard(CostTooHigh)`.
- `bound_role()` was `oo_r_picker`.
- `INSERT` returned `ExecError::Guard(Write)` and the binding stayed.
- `SELECT weight` as `oo_r_sorter` returned `ExecError::RoleReuse`.
- A new sorter session then read its column.

Oracle keys: `bound_role_after_guard=picker`, `role_reuse_after_guard=refused`.

The committed author test still requires the old result:

```575:575:crates/oo-gateway/tests/policy_catalog_pg_live.rs
assert_eq!(session.bound_role(), None);
```

That line sits after every `guard_only` call in `deny_hidden_write_and_injection`, including a successful path that would already have bound `oo_r_clerk`. A live run of that test against `78a60fde` panics there. Default `cargo test` does not enter the body, because the test returns unless `OO_A190_LIVE=1`.

The loader is consistent with the executor that ships in the same commit. This review does not ask for the bind to move back behind the guard.

## F2. Current generation is per source and digest

`ColumnAclPolicy.generation` says "maximum stored for this project" (`policy.rs` lines 91-92). The installer and the loader both use:

```sql
SELECT max(run_generation) FROM _oo.physical_mappings
WHERE source = $1 AND config_digest = $2
```

`policy.rs` lines 600-601. `policy_catalog.rs` lines 450-451.

Live, with digest `b200` generation 14 stored before the `a200` install:

| Policy digest | Generation | Result |
| --- | --- | --- |
| a200 | 3 | StaleGeneration |
| a200 | 14 | StaleGeneration |
| b200 | 9 | StaleGeneration |
| b200 | 14 | MappingAbsent |
| a200 | 9 | loaded |

Oracle key `max_generation_scope=source_and_digest`. A higher generation on another digest did not stale the current digest. The field comment is wider than the SQL. The SQL matches the catalog contract.

## F3. The post-commit xid check is weaker than an idle session

`ensure_committed` (`policy_catalog.rs` lines 1077-1090) accepts the client when `pg_current_xact_id_if_assigned()` is null.

The harness opened `BEGIN ISOLATION LEVEL REPEATABLE READ READ ONLY` on the admin client. During that transaction the xid function was null and a second admin session saw `xact_start` set. Oracle: `open_rr_xid_null=true`, `open_rr_xact_idle=false`.

After `ROLLBACK`, and after the loader returns checked by the harness, the second session saw `xact_start` null and `search_path` was not `pg_catalog`. Oracle keys ending in `=idle` record those returns, including `after_gap_rollback`, `after_refuse`, `after_zero`, `after_saturate`, the stale and missing-object paths, and `after_domain`.

The loader returned an idle client on this run. The xid predicate is also true for an open read-only transaction that has not assigned a transaction id. Commit and rollback themselves were the operations that cleared `xact_start`.

## F4. Mapping ambiguity was not produced through register

The loader returns `AmbiguousMapping` when the mapping count is not 1, or when the stored name disagrees with `lookup_physical_mapping` (`policy_catalog.rs` lines 986-1029). The primary key is `(source, config_digest, run_generation, level, logical_namespace, logical_table, logical_column)`. A second `register_physical_mapping` of the same identity returned `replay`. A different identity digest returned SQLSTATE `OO022`. The mapping count did not change. This review did not drop the primary key.

## F5. A database 42501 closes the dedicated session after rollback

Revoke `SELECT` on sealed, then run the previously loaded catalog through `DedicatedSession`. The stream returned `ExecError::Database` with SQLSTATE `42501`. `rollback_confirmed` was true. `is_open` was false. Oracle: `revoke_executor_rollback=true`, `revoke_executor_open=false`.

`publish_stop` calls `attempt_from_kind` with `socket_dropped` true (`query_executor.rs` lines 1277-1303). `reuse` is false. `run_producer` then drops the client and sets `open` false (lines 975-980). A successful star on a fresh session kept the socket: `query_rollback_confirmed=true` and the summary status was `Succeeded`.

The catalog connection is separate. Its idle checks stayed idle across the revoke.

## F6. Visibility, types, and the operator set matched the server

Picker pending was 2 (`token` and `_oo_pos`). Sorter pending was 6. Catalog order followed `attnum`, not the policy list. A later `GRANT SELECT` on token left pending at 2 and left the name out. Direct statements as `oo_a200_login` with `SET ROLE oo_r_picker` returned 42501 for token, sealed after revoke, the new column `extra`, and an insert. Guard refusals for hidden names did not include those names.

Published note altered to `public.a200_note` refused the whole load with `UnsupportedType`. The display text contained neither `note` nor `a200_note`.

Server OIDs: int2 21, int4 23, int8 20, text 25, bool 16, `count(*)` 2803. Fifty-four operators matched `pg_operator` for `<`, `<=`, `<>`, `=`, `>`, `>=` across bool/bool, int2/int2, int2/int4, int4/int2, int4/int4, int4/int8, int8/int4, int8/int8, and text/text. `pg_sleep`, `count(any)`, int4 `+`, and int2/int8 were on the server and absent from the catalog. `UnknownRows::{Refuse, Zero, Saturate}` were each explicit. No default was applied to -1.

Loader windows on `pg_stat_user_tables` had zero scan and tuple deltas. Final user row count was 3. Mapping count delta was 0.

## Outside this verdict

The live grantor was superuser `oo_fixture_admin`. Authentication, durable audit, row-level security, and a non-superuser grantor were not run. The audit hook only stored the admission in memory. SP06-W01 and SP06-W02 remain open. No parent plan item is closed. No release is claimed.
