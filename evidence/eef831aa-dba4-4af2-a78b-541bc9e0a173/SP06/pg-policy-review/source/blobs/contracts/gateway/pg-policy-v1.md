# Gateway column ACL

Contract `oo.gateway.pg-policy/v1`. Library `oo_gateway::policy`.

This is the column-ACL admission primitive for one mapped shadow table. A trusted administrator connection verifies the stored physical mapping and installs one `NOLOGIN` role with column `SELECT` grants. The caller supplies the session. The library does not accept a password argument on the install function, a raw `GRANT` script, or a row filter.

## What this slice does

`compile_column_acl` is pure. It refuses the whole input when a row filter is present, an identifier or the config digest is not closed, the allowlist is empty, a column is not `Confirmed` or `AutoAccepted`, the semantic type is empty, or PII is `Unknown` or declared without `explicit_allow`. That refusal happens before any database call.

`install_column_acl` requires a TLS session (`pg_stat_ssl.ssl` for this backend). It runs one transaction:

1. `session_user` must equal `current_user`, and neither may be `oo_target_owner`. The function does not `SET ROLE`.
2. `max(run_generation)` in `_oo.physical_mappings` for the project and config digest must equal the policy generation. A stale or missing generation returns before `CREATE ROLE`.
3. `_oo.lookup_physical_mapping` must return the `pt_` table and each `pc_` column. Names that are not 63-byte `pt_` / `pc_` hex identifiers are refused. Logical names are query parameters. Only those physical names are quoted into `GRANT`.
4. A new role is `CREATE ROLE ... NOLOGIN NOSUPERUSER NOCREATEDB NOCREATEROLE NOINHERIT NOBYPASSRLS NOREPLICATION`. An existing role must already have that shape and the same `SELECT` column set.
5. `GRANT role TO login WITH INHERIT FALSE, SET TRUE`. A missing login or a superuser login rolls the transaction back, including a role created earlier in that transaction.
6. A new role receives `USAGE` on schema `_oo` and `SELECT` on the allowlisted columns only. `information_schema.table_privileges` for that role and table must be empty. `PUBLIC` is not named in any grant.

`connect_require_tls` builds a rustls client with the aws-lc-rs provider, the supplied CA, and the server name on `TlsEndpoint.host`. `SslMode` is `Require`. There is no `NoTls` constructor and no verifier replacement. The password is passed only to the driver.

The role name is `oo_r_` plus a lowercase slug. The slug is at most 58 characters so the full name fits in 63 bytes.

## What stays open

Authentication of the administrator and of the login is open. This library does not issue credentials, store them, or check a caller beyond the PostgreSQL session it is given.

The audit WAL is open. Install does not write an audit row.

The gateway parser, portal execution, `CancelToken`, and the connection pool are open. `deadpool-postgres` is not a dependency of this slice. `pg_query` is not linked.

Row-level security and claims binding are open. A row filter is refused before any grant because `SET` of a custom GUC still succeeds after `REVOKE EXECUTE` on `set_config(text, text, boolean)` from `PUBLIC`. This slice does not claim an RLS proof.

Publication and the read API that would sit in front of this role are open. A column that arrives after install, including a column added to the shadow, stays invisible until a later policy names it and the classification rules pass. Unknown PII is not granted.

The tested grantor is the fixture superuser `oo_fixture_admin`. Membership `grantor` in `pg_auth_members` is that role. Column `attacl` may still display the table owner `oo_target_owner` as the grantor string. The library does not `SET ROLE oo_target_owner` to produce that display. A non-superuser grantor is not proven here. The caller must already be able to execute `_oo.lookup_physical_mapping` and to grant on the shadow.

The live test applies frozen `0000`, `0001`, `0003`, `0004`, and `0005` itself. It does not call the pipeline crate. `0002` is not applied. Schema install is the only place that `SET ROLE oo_target_owner`, matching the installer transaction, and that code is the test fixture.
