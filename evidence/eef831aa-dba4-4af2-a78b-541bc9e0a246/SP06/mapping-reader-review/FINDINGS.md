# Findings

Observations from the a246 review of fixed commit `1f1028f6161783d6e3bf1e9beb3a23a1161deca0`. None of these accept M1-D14E or release the gateway.

1. The smoke archive of that commit already contains `mapping_reader_access`. The review test was absent. Unit tests passed: 5 passed, 21 filtered out. The committed live test, with `OO_A240_LIVE` unset, returned in 0.00s and wrote no oracle. Lock content hash stayed `16e6afda29ae4bae251d569f3c4ad16459527f0d0323598d8e4e0971818d458a`.

2. `install_column_acl_set` for the two-table clerk set returned `Installed` and did not grant mapping read. `mapping_after_column_acl=absent`. `load_policy_catalog` as `oo_a246_login` returned `CatalogError::Database { sqlstate: "42501" }`. A direct `SELECT count(*)` on `_oo.physical_mappings` also returned 42501.

3. Refusals before any mapping grant: login argument mismatch (`LoginMismatch`), generation 3 against stored 7 (`StaleGeneration`), table name `missing` (`MappingAbsent`), and unknown login `oo_a246_missing` (`LoginAbsent`). Mapping privileges for `oo_a246_login` stayed absent after each refusal.

4. A second installed role, `oo_r_extra`, held only `USAGE` on `_oo`. `provision_mapping_reader` returned `UnexpectedPrivilege`. `SELECT` and lookup `EXECUTE` stayed false. The function did not revoke the `USAGE` it refused and did not grant the missing privileges. The product login was still absent.

5. Provisioning the clerk set returned `Installed`. The same call returned `Replay`. The `oo_r_clerk` column-grant text was unchanged. The login had zero column privileges on any table other than `_oo.physical_mappings`. `PUBLIC` held none of the mapping `USAGE`, `SELECT`, or lookup `EXECUTE` privileges.

6. After `Installed`, the login's `SELECT` on `_oo.physical_mappings` returned the same 14 rows the administrator sees. Two of those rows are `c|customer|private_note` and `c|customer|ratio`. The clerk policy does not name those columns. Table `SELECT` is not row-scoped. This review records that as metadata exposure. It does not record tenant isolation.

7. `information_schema.column_privileges` lists that table `SELECT` once per column of `_oo.physical_mappings`. The live test required that count to equal the mapping table's column count, and required shadow column privileges to be 0. Those catalog rows are the mapping table, not grants on a `pt_` relation.

8. The catalog load named `customer` and `order_doc`. The compiled query returned the three seeded customer rows, including null `display_name` and the text `丙号` and `丁铺`. `SET ROLE oo_r_clerk` then `SELECT` of `private_note` returned 42501, and `SELECT *` of the physical customer table returned 42501.

9. Source customer and order counts stayed 3. Reader `INSERT` returned 42501 before and after. Audit counts for `oo_a246_reader`, `oo_a246_login`, `oo_a246_runtime`, and `oo_pipeline_runtime` were 0. `pg_publication` count was 0. `pg_has_role(..., oo_target_owner, member)` was false for `oo_pipeline_runtime`, the runtime login, and the product login.

10. Attempt 1 did not reach PostgreSQL. The review harness compared `&&str` with `&str`. Exit 101. Dereferencing the iterator item is the harness fix. Product blobs were not changed.

11. Clippy `-D warnings` on `oo-gateway` lib, the review test, and the committed live test exited 0 in 1m 26s. The committed mapping reader sources and the review test contain no `unsafe` block.

12. The grantor in this fixture is `oo_fixture_admin`. A non-superuser grantor was not proven. Authentication of the administrator and of the login was not proven. The audit WAL, row-level security, and publication were not closed. The query-pg binary was not executed. The review called `load_policy_catalog` and `DedicatedSession::execute_trusted_select`.

13. The secret scan of the oracle, outcome, and test log found no password. Passwords and private keys stayed under `/tmp/oo-a246-eef831aa-dba4-4af2-a78b-541bc9e0a246/secrets`. The builder mount did not include `server.key` or `ca.key`. `cleanup/CLEANUP.txt` records removal of the a246 database, volumes, network, and private tmp directory. Images were not pruned. Other owners' containers were not removed.
