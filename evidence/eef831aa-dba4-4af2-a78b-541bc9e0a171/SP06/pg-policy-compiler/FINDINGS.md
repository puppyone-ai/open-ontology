# Findings

Source commit `d3429726618a4e69ada7fb35131f9043a21e23e9`. The deciding run is `linux/install-docker-run.exit` = 0 and `live/oracle.txt`.

## F1. Gateway driver

`oo-gateway` now depends on `tokio-postgres` 0.7.18, `postgres-types` 0.2.14, `postgres-protocol` 0.6.12, and `tokio-postgres-rustls` 0.14.0, plus the already locked `tokio`, `rustls`, and `tokio-rustls` lines. SQLx, the sync crate `postgres`, `oo-pipeline`, and `deadpool-postgres` are absent from `crates/oo-gateway/Cargo.toml`. The root `Cargo.toml` is unchanged.

`Cargo.lock` gained 24 pairs and lost none. `digest` 0.11.3 and `whoami` 2.1.3 kept their checksums and gained recorded dependency edges (`const-oid` 0.10.2, and the whoami target crates `libredox`, `objc2-system-configuration`, `wasite`). Details, dates, and license fields are in `supply/dependency-delta.md`. Every new crates.io version was created before 2026-09-28.

`tokio-postgres-rustls` is the TLS adapter required by `docs/07-gateway.md` and is not one of the four names in `is_gateway_postgres_stack`. The architecture gate was not edited. These three tests passed on the workspace after the fetch: `gateway_postgres_stack_is_allowed`, `gateway_sync_postgres_and_sqlx_are_forbidden`, `dev_only_gateway_pipeline_is_forbidden`. A name the taxonomy does not list is not an approval. `cargo deny` and `cargo audit` were not run. `fallible-iterator` 0.2.0 publishes the license string `MIT/Apache-2.0`. `wasi` 0.14.7 includes an `Apache-2.0 WITH LLVM-exception` alternative. `wasite` 1.0.2 includes a `BSL-1.0` alternative. This functional proof is not a supply-chain release approval.

## F2. Install behavior that was measured

`compile_column_acl` refuses a row filter, an open identifier, an empty allowlist, unknown PII, and a missing semantic type before any statement. The live refusals for a row filter and for unknown PII returned without creating `oo_r_` roles.

`install_column_acl` checks `pg_stat_ssl.ssl`, then one transaction. The grantor check requires `session_user = current_user` and refuses `oo_target_owner`. The tested session user was `oo_fixture_admin`. The function does not `SET ROLE oo_target_owner`.

Generation 3 against stored maximum 7 returned `StaleGeneration` before `CREATE ROLE`. A missing login returned `LoginAbsent` and the transaction rollback left `oo_r_orphan` absent. A second install of the same dispatcher policy returned `Replay`.

The shadow came from `_oo.full_import_prepare_shadow` on a mapping registered at generations 3 and 7, with the shadow built at generation 7. The fixture applied `0000`, `0001`, `0003`, `0004`, and `0005` from disk. `0002` was not applied.

`oo_r_dispatcher` received `SELECT` on `order_id` and `region`. `oo_r_clerk` received `SELECT` on `order_id` only. Both roles are `NOLOGIN` and `NOINHERIT`, granted to `oo_a171_login` with inherit false and set true. The membership grantor is `oo_fixture_admin`. Column `attacl` text shows grantor `oo_target_owner`, which is the table owner recorded by PostgreSQL when a superuser grants. The library did not `SET ROLE` to produce that string.

Dispatcher reads of the allowed columns returned 42 and west. Clerk read of `order_id` returned 42. Hidden `secret`, `_oo_pos`, a column added after install, `INSERT`, and `UPDATE` returned SQLSTATE `42501` in a read-write transaction. Metadata `SELECT` on `_oo.physical_mappings` and `_oo.full_import_run` returned `42501`. The login without `SET ROLE` returned `42501`. Two roles used backend pid 175, and that pid was the same after each `ROLLBACK`. Reader TLS was observed from the administrator session.

`relacl` on the shadow names `oo_target_owner` and `oo_pipeline_runtime` insert, not the reader and not `PUBLIC`. Table privileges for the role, `PUBLIC`, and the login are empty.

## F3. What this slice leaves open

Authentication of the administrator and of the login is open. The library uses the PostgreSQL session it is given. It does not issue or store credentials.

The audit WAL is open. Install writes no audit row.

The gateway parser, portal, `CancelToken`, and pool are open. `deadpool-postgres` was not added. `pg_query` is not linked.

Row-level security and claims binding are open. A row filter is refused because a169 measured that `REVOKE EXECUTE` on `set_config(text, text, boolean)` from `PUBLIC` does not stop `SET LOCAL oo.claims`. This slice does not claim an RLS proof.

Publication and the read API in front of the role are open. A column added after install stayed invisible.

The tested grantor is superuser `oo_fixture_admin`. A non-superuser grantor is not proven. Superuser bypasses ordinary `GRANT` and `EXECUTE` checks, including `EXECUTE` on `_oo` functions revoked from `PUBLIC`. The caller must already be able to execute `_oo.lookup_physical_mapping` and to grant on the shadow.

Schema install in the test is the only `SET ROLE oo_target_owner`. That is fixture code, not the policy API.

## F4. Preserved failures

Attempt 1 failed clippy before any database call. Attempt 2 reached `adopt_target_generation` and failed the uuid bind. Attempt 3 failed `CREATE ROLE` because roles survive `DROP DATABASE`. Attempt 4 failed the numeric bind for `_oo_pos`. Those logs remain under `linux/attempt1-clippy`, `linux/attempt2-uuid-bind`, `linux/attempt3-role-collision`, and `linux/attempt4-numeric-bind`. The green log is `linux/install-docker-run.log`.
