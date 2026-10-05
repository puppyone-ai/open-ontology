# Findings

Independent review of `contracts/gateway/column-acl-set-v1.md` blob `1bc75704e7930fea79b743c55a9f0721697fa3e2` and `crates/oo-gateway/src/policy.rs` blob `84a86e85ef82bc2a459a5e2faef5827d077ca6cc` at `543580768dec60d127eb428c09e0a9a400b18272`. Verdict: APPROVE_LIMITED.

This is a review of the existing column-ACL component's set installer. It is not a new component and it is not a first-review count. M1-D14A stays `未验收a218`. No ledger line was edited.

## Contract behavior that the green run showed

`compile_column_acl_set` runs before TLS and before SQL. The live pure set refused an empty list, a duplicate logical table, mixed identity, a row filter, a metadata name, unknown classification, and the 32-table / 512-column bounds, with `pure_grant_events=0`. The library unit test `a_set_refuses_empty_mixed_duplicate_bounds_and_metadata` passed on the pinned 1.94 image, 6 policy tests in all. `compile_column_acl` still accepts a logical `_oo_pos`; the set compiler refuses it. The unit test covers that split. The live fixture did not send `_oo_pos` as a set column.

Inside the transaction, and before `CREATE ROLE`, the set path checks the grantor, the current generation for this project and digest, `pt_` / `pc_` lookup, distinct physical tables, and column types (`bool`, `int2`, `int4`, `int8`, `text`). The second policy with no mapping returned `MappingAbsent`. The second policy with logical column `ratio` (`float4`) returned `TypeRefused`. Both roles were absent, both grant-event deltas were 0, and both timings are `pre_create_role`. A missing login returned `LoginAbsent` with zero grant events. Digest B at generation 11 left digest A at 7. A stale generation on digest A returned `StaleGeneration` with zero grant events.

A new role is created once, then `GRANT role TO login WITH INHERIT FALSE, SET TRUE`, then per table `GRANT USAGE` on `_oo` and column `SELECT`. Scribe and glance recorded `Installed`. Exact replay recorded `Replay`. The old one-table API, called after the set install, recorded `Replay` and left the second table's grants unchanged.

An existing role plus a different physical table still returns `GrantMismatch` from `install_column_acl` and does not add a grant (`legacy_second_table=GrantMismatch`, `legacy_counter_columns=1`). Installing the two-table set onto that partial role also returns `GrantMismatch` (`legacy_set_widen=GrantMismatch`). The set API does not widen the old function.

An existing role is accepted only for the exact column set. A partial list and a changed column returned `GrantMismatch`. An extra `ratio` grant was left in place by the installer (`unexpected_grant=left_in_place`). The review test revoked that extra grant after the assertion; the installer did not. A different member returned `MembershipMismatch`, and the grant text matched the pre-change snapshot. Observed shape was `NOLOGIN`. Membership was inherit false, set true, admin false. The grantor was `oo_fixture_admin`. Table privileges were 0. `_oo_pos` was not selectable. Schema usage was `USAGE` without `CREATE`.

The source calls `grant_membership` again on the new-role path after the existence check, including when the role already existed and the earlier branch returns first. The existed path returns from `ensure_exact_grants` before that second `grant_membership`. The live replay did not add columns. This review did not count GRANT events on the replay call itself.

`query-pg` does not install grants. Scribe exit 0 covered both tables, `count(*)`, and star. Glance exit 0 omitted the ungranted values (`glance_hidden=absent`). The guard exit was 6, stderr `query-pg: guard`, and no guard audit file. Direct hidden reads, writes, and raw `SELECT *` were `42501` on a stable reader backend.

`reltuples` stayed unknown through install. The test, not the installer, ran `ANALYZE`. `reltuples_after=known`. `publication=0`. `authentication=not_claimed`. `rls=open`. `audit_wal=open`.

## Post-grant rollback

The author live proof stops at `MappingAbsent` and `TypeRefused`, both before `CREATE ROLE`. The contract also says a database error during the post-grant reads rolls back the new role and the grants. This review injected one fault after grants had started.

The injection is a public event trigger, `ddl_command_end` when the tag is `GRANT`. It is installed by the review test, not by product code. `nextval` on `a229_grant_seen` survives the aborted transaction. The scribe install counted 4 GRANT events. The membership grant did not appear in that count. The four events match two `grant_usage_and_columns` batches, and each batch is `GRANT USAGE` plus column `SELECT`.

`fail_at` was that count. The fault role's install raised sqlstate `P0001` when the counter reached 4, so three earlier GRANT trigger calls had returned in the same transaction. The product maps the driver error with `Error::to_string()`, so `PolicyError::Database.message` is `db error` and the server text is not in the `Result`. That matches the driver. The contract asks the display text to omit passwords and GRANT scripts. It does not require the server exception text to be preserved.

`pg_event_trigger_ddl_commands()` on that command returned `object_type=TABLE` and empty `objid`, `object_identity`, and `relname`. Attempt 2 stopped on that empty name and did not reach the rollback checks. The green run also read `current_query()`. The query text was the second table's batch, schema `USAGE` and column `SELECT` on that table's `pt_` name. The non-transactional marker `a229_second_hit` became 1 only for that match. Oracle `post_grant_target=second_table` and `post_grant_second_hit=1` mean that query match. They do not mean `object_identity` contained the name.

After the error the admin session still ran `SELECT 1` (`same_backend_after_fault=true`). `oo_r_fault` was absent. Its column privileges were 0. The scribe grant text was unchanged. `post_grant_events=4`, equal to the successful install. That is the post-grant rollback. The pre-create-role mapping and type failures stay on their own oracle lines and stay at zero grant events.

## Limits

- The tested grantor is fixture superuser `oo_fixture_admin`. A non-superuser grantor was not proven. The contract already leaves that open. This review does not make it a mandatory later gate.
- The catalog `USAGE` / `SELECT` / `EXECUTE` grant to the login is fixture code, applied after the hook is dropped. It is distinct from the installer.
- Row-level security, administrator and login authentication, publication, and the audit WAL remain open. The installer writes neither an audit row nor `pg_publication`.
- Two logical tables that resolve to one physical table are specified as `InvalidIdentifier { field: "table" }` after lookup. This fixture used two distinct physical tables and did not add a collision case.
- `has_schema_privilege` on `public` can be true through `PUBLIC`. The check that passed is `_oo` `USAGE` without `_oo` `CREATE`.
- The event trigger cannot see a membership `GRANT` in this server: the install count was 4, not 5. The rollback proof is the column and schema grants, which are the statements that create the table privileges.
- Attempt 1 and attempt 2 are preserved under `linux/proof/attempt-1/` and `linux/proof/attempt-2/`. They are test-oracle gaps, not a role left behind.
- `policy.rs` has no `unsafe` token. `lib.rs` contains `forbid(unsafe_code)`. Clippy `-D warnings` on the library, the review test, and the frozen author test exited 0.
- `Cargo.lock` was not edited. No dependency was added. a207 remains the connector manifest and root-lock writer.
