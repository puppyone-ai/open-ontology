# Gateway column ACL set

Contract `oo.gateway.column-acl-set/v1`. Library `oo_gateway::policy`.

This is the aggregate column-ACL admission primitive. A trusted administrator connection installs one `NOLOGIN` role for a bounded list of mapped shadow tables in one transaction. The caller supplies the session. The library does not accept a password argument, a raw `GRANT` script, or a row filter.

`install_column_acl` in `contracts/gateway/pg-policy-v1.md` is unchanged. It still installs or replays one table. When that role already exists, a different physical table still returns `GrantMismatch` and adds no grant. This contract does not widen that function.

## Input

`compile_column_acl_set` and `install_column_acl_set` take `&[ColumnAclPolicy]`. Each element is the existing one-table policy. The list is the full set for one role.

The list has 1 to 32 policies. Each policy's column list has 1 to 512 columns. Zero policies is `EmptyPolicySet`. A longer list, or a wider column list, is `LimitExceeded`. Those checks are pure.

Every policy must share `project`, `config_digest`, `generation`, `role`, and `login`. A difference is `MixedIdentity`. The same logical namespace and table twice is `InvalidIdentifier { field: "table" }`. A column named `_oo_pos`, or any logical name that starts with `_oo_`, is `MetadataColumn`. `compile_column_acl` still accepts `_oo_pos` as a logical name; only the set compiler refuses it.

Each element is then compiled with `compile_column_acl`. A row filter, an open identifier, an empty allowlist, a duplicate column, an unclassified column, an empty semantic type, unknown PII, or declared PII without `explicit_allow` refuses the whole set. No database call has been made.

## Install

`install_column_acl_set` requires a TLS session (`pg_stat_ssl.ssl` for this backend). It runs one transaction. The order is fixed:

1. `session_user` must equal `current_user`, and neither may be `oo_target_owner`. The function does not `SET ROLE`.
2. `max(run_generation)` in `_oo.physical_mappings` for this project and this config digest must equal the policy generation. A later generation stored under another digest does not raise this maximum. A stale or missing generation returns before `CREATE ROLE`.
3. `_oo.lookup_physical_mapping` must return every `pt_` table and every `pc_` column. Logical names are query parameters. Only those physical names are quoted into `GRANT`. Two policies that resolve to one physical table are refused. A missing mapping is `MappingAbsent`.
4. Each mapped column must exist and its `pg_type` must be a base type in `bool`, `int2`, `int4`, `int8`, or `text`. Any other type, including `float4`, is `TypeRefused` and names the logical column. This check finishes before a grant.
5. The login must exist and must not be a superuser. A missing or superuser login is `LoginAbsent`.
6. A new role is `CREATE ROLE ... NOLOGIN NOSUPERUSER NOCREATEDB NOCREATEROLE NOINHERIT NOBYPASSRLS NOREPLICATION`, once. `GRANT role TO login WITH INHERIT FALSE, SET TRUE` runs once. Each table then receives `GRANT USAGE` on schema `_oo` and `GRANT SELECT` on its allowlisted columns only.
7. An existing role is replayed only when its attributes already match that shape, its membership is exactly this login with inherit false, set true, and admin false, `USAGE` on `_oo` is present, `CREATE` on `_oo` is absent, and the role's column privileges across every schema are exactly the mapped columns of this full set with `SELECT` only. `information_schema.table_privileges` for the role must be empty, so a table-wide `SELECT` does not pass. `_oo_pos` must not be selectable. Anything else is `GrantMismatch` or `MembershipMismatch`. The function does not revoke the unexpected grant and does not add the missing one.

After a new install, the same shape, membership, usage, column set, empty table-privilege set, and hidden `_oo_pos` are read back. A failed read rolls the transaction back.

`CREATE ROLE` is inside the transaction. One bad table, whether missing, mistyped, or rejected by the post-grant read, rolls back the new role and every grant made for an earlier table in that call.

The outcome is `SetInstallOutcome`. `effect` is `Installed` or `Replay`. `tables` follows the input order and carries the physical table and physical columns.

## What stays open

Authentication of the administrator and of the login is open. This library does not issue credentials or store them.

The audit WAL is open. Install does not write an audit row.

Row-level security, claims binding, and publication are open. A row filter is refused before any grant. This contract does not claim an RLS proof. `pg_publication` is not written.

`query-pg` is a separate command. It does not install grants. After this install, a caller can load the same policy list through that command. `reltuples` of `-1` still refuses a catalog load whose `unknown_rows` is `refuse`. This installer does not `ANALYZE`.

A column that arrives after install, including `_oo_pos` and a column absent from the set, stays ungranted. Unknown PII is not granted.

The tested grantor is the fixture superuser `oo_fixture_admin`. A non-superuser grantor is not proven here.

The live test applies frozen `0000`, `0001`, `0003`, `0004`, and `0005` itself. It does not call the pipeline crate. `0002` is not applied. Schema install is the only place that `SET ROLE oo_target_owner`, and that code is the test fixture.

This contract does not close SP06-W01. It does not accept a parent plan item and it does not release.
