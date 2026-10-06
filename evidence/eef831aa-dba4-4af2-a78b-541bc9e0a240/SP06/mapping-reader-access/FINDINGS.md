# Findings

Observations from the a240 mapping-reader run. None of these accept a parent plan item or release the gateway.

1. The fixed archive does not contain `mapping_reader_access`. Smoke of `df0bce2e235babc0944b68350921c13038741ab9` refused the overlay, compared `policy.rs` to the archived blob, and ran `policy::tests`: 6 passed. Lock hash stayed `16e6afda29ae4bae251d569f3c4ad16459527f0d0323598d8e4e0971818d458a`.

2. `install_column_acl_set` still does not grant the login `USAGE` on `_oo`, `SELECT` on `_oo.physical_mappings`, or `EXECUTE` on lookup. After both roles were installed, `mapping_after_column_acl=absent`. `load_policy_catalog` as `oo_a240_login` then returned `CatalogError::Database { sqlstate: "42501" }`.

3. Provisioning the clerk set returned `Installed`. Provisioning the same login again, and provisioning the peek set on that login, returned `Replay`. Role column-grant text for `oo_r_clerk` and `oo_r_peek` was unchanged across the grant. The login gained no column privilege on a shadow table.

4. Table `SELECT` on `_oo.physical_mappings` is visible in `information_schema.column_privileges` once per mapping column. Attempt 2 failed `login_column_grants == 0` with left 13 and right 0. Those 13 rows are the mapping table, which has 13 columns. The test now requires shadow column privileges to be 0 and mapping-column `SELECT` privileges to equal the mapping column count. The contract names this catalog listing. It is metadata exposure, not scoped isolation.

5. Attempt 1 did not reach PostgreSQL. `prepare(...).expect_err(...)` needs `Debug` on `Prepared`. The struct had no `Debug`. Exit 101. `#[derive(Debug)]` on `Prepared` and `PreparedTable` is the fix. Unit tests then passed: 5 passed.

6. Attempt 3 called `init-pg` on a cluster that already had `oo_target_owner` and `oo_pipeline_runtime` from attempt 2. Exit 6, stderr `init-pg: target catalog refused`, stdout empty. That matches the init contract: a second empty database on a cluster that already has those roles is refused. The fixture reset now revokes members, drops owned objects, and drops those two roles before creating the databases again. The final run returned `install=bootstrapped`.

7. Refusals on the final run, with no mapping grant left behind: empty set, row filter, login mismatch, missing role (`MembershipMismatch`), stale generation 3 against stored 7, missing table (`MappingAbsent`), unknown login and superuser login (`LoginAbsent`), member of `oo_target_owner` (`OwnerMembership`), `float8` column `ratio` (`Unsupported`), one-table subset (`GrantMismatch`), and a login that already held only `USAGE` (`UnexpectedPrivilege`, `SELECT` and `EXECUTE` stayed false). `oo_r_wide`, `oo_r_super_set`, and `oo_r_member_set` were not created.

8. Compiled queries returned the seeded rows. Clerk customer: 41 `丙号` true, 42 null false, 43 `丁铺` true. Peek orders: 701 with int8 minimum, 702 with int8 maximum, 703 with 0. Direct `SELECT` of `private_note`, `ratio`, peek `clerk_note`, and `SELECT *` returned `42501`.

9. Source writes stayed at the administrator seed. Customer and order counts stayed 3. Reader `INSERT` returned `42501` before and after. Audit counts for `oo_a240_reader`, `oo_a240_login`, `oo_a240_runtime`, and `oo_pipeline_runtime` were 0. `oo_fixture_admin` had at least 6 audit rows. `pg_publication` count was 0. Runtime owner membership was false before and after.

10. `PUBLIC` held none of the mapping `USAGE`, `SELECT`, or lookup `EXECUTE` privileges after provisioning. The product login was not a member of `oo_target_owner`. No source grant and no owner membership was added.

11. Clippy `-D warnings` on `oo-gateway` lib and `mapping_reader_access_pg_live` exited 0 in 1m 25s. The final live compile emitted no warnings. Attempt 2 had unused `mut` and unused `admin` parameters; those were removed before the final run.

12. The chain secret scan of the oracle, outcome, and test logs found no password. A later scan of the evidence tree against every secret file found no password, key, or certificate. The only name match was the public `DNS:oo-a240-pg` line inside `pg/up.sh`. Passwords stayed in `/tmp/oo-a240-eef831aa-dba4-4af2-a78b-541bc9e0a240/secrets` and were not written into the report. `cleanup/CLEANUP.txt` records removal of that directory, `oo-a240-pg`, the three a240 volumes, and the a240 network. Images were not pruned. Other owners' containers were not removed.

13. Other tasks had builders and databases during this run (`oo-a241-*`, `oo-a242-*`, and earlier `oo-a236-pg`). They were not stopped, resized, or deleted. This task's builder count stayed at one, at 6 GiB and 1 CPU.
