# Findings — a169 policy compiler

## F1. A real install client cannot be added to oo-gateway in this slice

`crates/oo-arch-check/src/policy.rs` reserves `tokio-postgres`, `postgres-types`, `postgres-protocol`, and `deadpool-postgres` for the gateway (`is_gateway_postgres_stack`). `is_gateway_forbidden_driver` rejects the SQLx family and the synchronous `postgres` crate. The required gateway edge is only `oo-gateway` to `oo-core`. The three focused tests of that policy passed on the pre-edit tree (`linux/phases/arch.exit` = 0).

The root `Cargo.lock` contains `sqlx` `0.9.0` and has no `tokio-postgres` package (`linux/lock-oracle.txt`). This task allows gateway dependency bookkeeping only for already resolved `sqlx`, `tokio`, `serde`, `serde_json`, and `sha2`, and it forbids new versions and `cargo update`. A dev-dependency from `oo-gateway` to `oo-pipeline` is the edge `dev_only_gateway_pipeline_is_forbidden` rejects.

An install function that opens a PostgreSQL session needs a client inside the `oo-gateway` package. SQLx is the resolved client and the gate forbids it there. `tokio-postgres` is the client the gate allows and it is absent from the lockfile. Changing the gate, the lockfile set, or the migrations is outside this slice. A function that reports success without applying grants would claim an admission primitive that does not exist.

No `src/policy.rs`, no live compiler test, no `contracts/gateway/pg-policy-v1.md`, and no `oo-gateway` manifest change were added.

## F2. Row-filter policies stay refused

On this PostgreSQL 17.10 image, `REVOKE` of `pg_catalog.set_config(text, text, boolean)` from `PUBLIC` holds for `oo_r_a`: the call returns SQLSTATE `42501`. The same role can still run `SET LOCAL oo.claims = '{"region":["us"]}'`. The statement produced no error, and `current_setting('oo.claims', true)` returned the assigned value (`pg/out/utility_set.stdout`, empty stderr).

`RESET ROLE` returns to the session user. That login holds `EXECUTE` on `set_config`, so it can replace the value, and the next `SET LOCAL ROLE oo_r_a` observes the replacement (`pg/out/reset_role.stdout`). A custom GUC written before `SET LOCAL ROLE` is not sealed to that role change.

This slice does not add parameter ACLs or a security-definer claims seal. Those are broader than the reviewed schema. A policy that contains a row filter therefore has no enforceable proof here. The open row-level security requirement stays open. A later compiler must reject the whole policy when a filter is present, including its column grants.

## F3. Column grants work for a fixture superuser, on a table that is not a reviewed shadow

The fixture administrator, which is a superuser and is not `oo_target_owner`, issued column `GRANT SELECT` on `_oo.probe_orders`. Allowed columns returned the inserted rows. Hidden column `secret`, column `extra` added afterward by the owner, and `INSERT` in a read-write transaction failed with SQLSTATE `42501`. Two roles on one backend did not keep each other's column rights across `ROLLBACK`. The login has no object grant of its own, so `RESET ROLE` does not widen the column ACL. Catalog details are in `REPORT.md` and `pg/out-sqlstate/admin.stdout`.

PostgreSQL reported `permission denied for table probe_orders` for the hidden column, the new column, the other role's column, and the insert. The column boundary is the `attacl` contents together with which select lists returned rows.

`_oo.probe_orders` was created with `CREATE TABLE` by `oo_target_owner`. `_oo.full_import_prepare_shadow` was not called, and no mapping row binds the probe table. Import metadata tables exist from `0005` and `oo_r_a` cannot read them. That supports the privilege shape. It does not prove grants against a shadow produced by the reviewed prepare function.

Attempt 1 is preserved because `LOCK TABLE` in `0003` requires the transaction the installer opens. The migration files were not changed to make attempt 2 pass.

## Prerequisites for a later slice

1. Admit the gateway postgres stack in the root lockfile at reviewed versions, or change the architecture gate in its own reviewed slice. This slice does neither.
2. Keep SQLx off `oo-gateway`, including dev-dependencies and transitive edges.
3. Keep `oo-pipeline` off `oo-gateway`. A gateway test can apply the reviewed SQL files through the fixture administrator, as this probe did.
4. Install column ACLs from a superuser, or another grantor, that is a different role from `oo_target_owner`. Do not `SET ROLE oo_target_owner` to make the policy install look like the schema owner.
5. Create a `NOLOGIN` role with no superuser, bypassrls, createrole, createdb, or replication attribute. Grant it to an existing login `WITH INHERIT FALSE, SET TRUE`. Grant schema `USAGE` and column `SELECT` only.
6. Reject any policy input that contains a row filter until a server-enforced claims binding exists. F2 is the current measurement of that gap.
7. Prove the grants on a shadow created by the reviewed prepare function, with a new column added after install, a read-write `42501` for writes, and two roles on one backend.
