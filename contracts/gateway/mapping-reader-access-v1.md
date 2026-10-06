# Gateway mapping reader access

Contract `oo.gateway.mapping-reader-access/v1`. Library `oo_gateway::mapping_reader_access`.

This is administrator-selected provisioning of one target login. A trusted administrator TLS session and an explicit login accompany one full column-ACL policy set. The function grants that login the mapping read `load_policy_catalog` and `query-pg` need, and nothing else.

It does not authenticate the administrator or the login. It does not decide tenancy. It does not isolate one tenant's rows from another. `SELECT` on `_oo.physical_mappings` is table-level, and `_oo.lookup_physical_mapping` is `SECURITY DEFINER`. After a successful call the login can read every mapping row in that database and look up any stored identity, including rows outside the policy set. Those rows carry the source, config digest, generation, logical names, and physical `pt_` / `pc_` names. That is metadata exposure. It is not scoped database isolation.

`install_column_acl` and `install_column_acl_set` are unchanged. They still grant column `SELECT` and schema `USAGE` to the `NOLOGIN` role, with `INHERIT FALSE` on the login. They do not grant this login `USAGE` on `_oo`, `SELECT` on `_oo.physical_mappings`, or `EXECUTE` on `_oo.lookup_physical_mapping`. This function does not grant a business column, a table-level `SELECT`, or `EXECUTE` on `_oo.register_physical_mapping`.

## Input

`provision_mapping_reader` takes the administrator `Client`, the login name, and `&[ColumnAclPolicy]`.

`compile_column_acl_set` runs first. An empty set, a mixed identity, a row filter, a metadata column, an open identifier, an unclassified column, or a bound violation returns that `PolicyError` and does not open a transaction. The login argument must equal the compiled login. A difference is `LoginMismatch` and does not open a transaction.

## Install

The client must already be a TLS session (`pg_stat_ssl.ssl` for this backend). One transaction then runs, in this order:

1. `session_user` must equal `current_user`, and neither may be `oo_target_owner`. The function does not `SET ROLE`.
2. `max(run_generation)` in `_oo.physical_mappings` for this project and config digest must equal the policy generation. A stale or missing generation is `StaleGeneration`. A later generation stored under another digest does not raise this maximum.
3. `_oo.lookup_physical_mapping` must return every `pt_` table and every `pc_` column. A missing row is `MappingAbsent`. A stored name that is not a 63-byte `pt_` / `pc_` hex identifier is `PhysicalAbsent`. Each mapped column must be a base type in `bool`, `int2`, `int4`, `int8`, or `text`. Any other type is `Unsupported` and names the logical column.
4. The login must exist, must not be a superuser, and must not be `oo_target_owner`. Otherwise the result is `LoginAbsent`. Membership of the login in `oo_target_owner` is `OwnerMembership`. The function does not grant that membership and does not revoke it.
5. The `oo_r_<slug>` role must already exist with the set installer's `NOLOGIN` shape. Its only membership must be this login with inherit false, set true, and admin false. A missing role or a different membership is `MembershipMismatch`. A different attribute is `RoleMismatch`.
6. The role's column privileges must already be exactly this full set: `SELECT` on the mapped columns, `USAGE` on `_oo`, no `CREATE` on `_oo`, no table-level privilege, and no `SELECT` on `_oo_pos`. Anything else is `GrantMismatch` or `MetadataColumn`. The function does not revoke or add a business grant.

Only after those checks does the function look at the login's own mapping privileges:

| Object | Accepted privilege | Refused |
| --- | --- | --- |
| schema `_oo` | `USAGE` | `CREATE` |
| `_oo.physical_mappings` | `SELECT` | `INSERT`, `UPDATE`, `DELETE`, `TRUNCATE`, `REFERENCES`, `TRIGGER` |
| `_oo.lookup_physical_mapping(text, text, bigint, text, text, text, text)` | `EXECUTE` | |
| `_oo.register_physical_mapping(text, text, bigint, text, text, text, text, bytea)` | none | `EXECUTE` |

`PUBLIC` must hold none of those privileges. The absent state is all three accepted privileges false and no refused privilege. The function then grants exactly the three accepted privileges to the named login and reads them back. That result is `Installed`. The exact state is `Replay` and issues no `GRANT`. Any partial or extra privilege is `UnexpectedPrivilege`. The function does not revoke it and does not add the missing piece.

`information_schema.column_privileges` lists the table `SELECT` on `_oo.physical_mappings` once for each column of that table. Those rows are the mapping catalog. They are not grants on a shadow table. The login receives no column privilege on a `pt_` relation.

`GRANT` is inside the same transaction. A failed read rolls the new grants back. No statement grants a source schema, a source table, `PUBLIC`, `oo_target_owner`, or a shadow column.

The outcome is `MappingReaderOutcome`. `effect` is `Installed` or `Replay`. `login` and `role` are the names that were checked.

## What stays open

Authentication of the administrator and of the login is open. This function does not issue a credential, read a password, or store one.

The audit WAL, row-level security, and publication are open. Provisioning writes no audit row and no `pg_publication` row.

A non-superuser grantor is not proven here. The live fixture uses `oo_fixture_admin`.

The live test calls public `init-pg` and `import-pg`, then `install_column_acl_set`, on one private TLS cluster. It does not edit migrations. Two imported tables and two installed roles are the success fixture. Before provisioning, `load_policy_catalog` as the login returns SQLSTATE `42501`. After provisioning, the catalog load and one compiled query succeed for each role. A direct `SELECT` of a column the role was not granted returns `42501`. Source `INSERT` by the source reader stays `42501`, and the source audit trigger records no write by that reader, the runtime login, or the product login. `oo_pipeline_runtime` is not a member of `oo_target_owner`.

This contract does not close SP06-W01. It does not accept a parent plan item and it does not release.
