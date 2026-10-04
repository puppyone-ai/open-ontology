# a169 report — database policy compiler first slice

Task `a169`. Session `eef831aa-dba4-4af2-a78b-541bc9e0a169`. Repository `/Users/redux/puppy-one/open-ontology`. Fixed base `3dadf678ff0fbfdc1ddd2ac4144ee68cb2ba772c` is an ancestor of the tree that was tested. Work started at `d9797edd63b976c1fb7ab823b9103ef347b192d4`.

This slice records a blocking finding. `oo-gateway` has no policy install API. No file under `crates/oo-gateway`, `contracts/gateway`, the workspace `Cargo.toml`, or `Cargo.lock` was edited. The migrations, installer, and import functions were not edited.

## Pre-edit smoke

One Linux arm64 container, image `docker.io/library/rust@sha256:0f24958fa693fe838859faf64a0ec78d382cd543a6b6efd0fa7a3bcb10ea90d5`, `RUSTUP_TOOLCHAIN=1.94.0`. Private bridge `oo-a169-net` (`internal=false`, so `cargo fetch` and `rustup component add` could reach the network). Cap drop `ALL`, no new privileges, no host network, no published port, no host home, cargo and target on task volumes. The repository mount was read-only.

`rustc 1.94.0 (4a4ef493e 2026-03-02)`. Clippy for 1.94.0 was added inside that container (`linux/phases/clippy-component.exit` = 0).

| Phase | Exit |
| --- | --- |
| `cargo fetch --locked` | 0 |
| `cargo test --locked --offline -p oo-gateway --lib` | 0 (0 tests) |
| `cargo clippy --locked --offline -p oo-gateway --lib -- -D warnings` | 0 |
| `gateway_postgres_stack_is_allowed` | 0 |
| `gateway_sync_postgres_and_sqlx_are_forbidden` | 0 |
| `dev_only_gateway_pipeline_is_forbidden` | 0 |
| worst | 0 |
| `docker run` | 0 |

`linux/lock-oracle.txt` records `lock_has_tokio_postgres=no` and the first `sqlx` version line `0.9.0`. Raw output is `linux/docker-run.log`. The container was not privileged and was not OOM-killed (`linux/inspect.txt`).

## Database measurement

One PostgreSQL container, image `docker.io/library/postgres@sha256:3aac91d19d5b7aff4c650dddfacc1a26c436459aafd4eb51ab0bf1748e990dbb`. `pg/inspect.txt`: running, not privileged, network `oo-a169-net`, no host port binding. Admin password and the reader password lived in `/tmp/oo-a169-eef831aa-dba4-4af2-a78b-541bc9e0a169/secrets` and were removed during cleanup. The container environment had `POSTGRES_PASSWORD_FILE` and did not carry the password as a variable. Reader sessions used `sslmode=verify-full` against DNS `oo-a169-pg`.

`pg/out/tls_identity.stdout`: user `oo_a169_login`, TCP true, SSL true, `PostgreSQL 17.10 (Debian 17.10-1.pgdg12+1)` on `aarch64-unknown-linux-gnu`.

Attempt 1 applied the migration files with autocommit. `0003_target_segment_preflight_expand.sql:133` failed with `LOCK TABLE can only be used in transaction blocks` (`pg/attempts/1/out/install.stderr`, exit 3). That output is kept. Attempt 2 followed the transaction shape of `oo_pipeline::target_installer`: `0000` outside a transaction (it creates roles), then `0001`, `0003`, and `0004` in one transaction as `oo_target_owner`, then `GRANT pg_read_all_stats TO oo_target_owner WITH INHERIT TRUE, SET FALSE` as the fixture superuser, then `0005` in a second transaction as `oo_target_owner`. SHA-256 of the files on disk:

| File | SHA-256 |
| --- | --- |
| `0000_bootstrap_roles.sql` | `37d745803589ba48cde1703a4ed7c4c63a956c7c5074aa618851d09db1e26208` |
| `0001_target_fence_expand.sql` | `97a5bad069392988a4ee5f7b58a7bc526885b17aa26c606566aa4c8671fb96e9` |
| `0003_target_segment_preflight_expand.sql` | `d046d98b825dbf45aa5a60c886fc47e7dd1fcbce074a5dace5a75bd0693e15a3` |
| `0004_physical_mapping.sql` | `844de6f645f8f1ebf29d05118ae25d4669d8dc870eed1fa7697d4b98456eaf95` |
| `0005_full_import_expand.sql` | `a9cef2013019784857353ff4fe060350d71a4e9ac4ab18551b992a7a9b52ecb6` |

Those digests were inserted into `_oo.schema_migrations`. This was `psql` applying the reviewed SQL files. It was not a Rust call to `install` or `install_full_import`.

The probe table is `_oo.probe_orders`, created by `oo_target_owner` after the migrations. It is not a shadow from `_oo.full_import_prepare_shadow`. No physical-mapping row was registered for it.

The fixture superuser then created `NOLOGIN` roles `oo_r_a` and `oo_r_b`, granted them to `oo_a169_login` `WITH INHERIT FALSE, SET TRUE`, granted schema `USAGE` to those roles, and granted column `SELECT` only: `(id, region)` to `oo_r_a` and `(id)` to `oo_r_b`.

Measured results, with raw files under `pg/out/` and SQLSTATE under `pg/out-sqlstate/`:

- Repeatable-read read-only as `oo_r_a` returned rows `(1, eu)` and `(2, us)`.
- `SELECT secret`, `SELECT extra`, `SELECT region` as `oo_r_b`, and `INSERT` as `oo_r_a` each returned SQLSTATE `42501`, `permission denied for table probe_orders`, at `aclchk.c:2843`. The insert ran in a read-write transaction.
- `attacl` grants `id` to `oo_r_a` and `oo_r_b`, and `region` to `oo_r_a`. `secret` and `extra` have empty column ACLs. `probe_orders.relacl` is null. The stored grantor on the column ACLs is `oo_target_owner`. The session that issued the `GRANT` had reset to `oo_fixture_admin`.
- Backend pid `182` ran `oo_r_a`, rolled back, ran `oo_r_b` (ids returned, region denied), and was `oo_a169_login` again on that same pid. A later reader session, pid `295`, stayed on one backend through the SQLSTATE script and returned to `oo_a169_login`.
- `oo_a169_login` with no `SET ROLE` is denied schema `_oo`. `oo_r_a` has schema `USAGE` and is denied `SELECT` on `_oo.physical_mappings` and `_oo.full_import_run` (`42501`). `has_table_privilege` is false for `oo_r_a` on `full_import_session` and `full_import_table` as well.
- Role flags: `oo_r_a` and `oo_r_b` are not superuser, not login, not inherit, not bypassrls, not createrole, not createdb, not replication. They are not members of `oo_target_owner` or `pg_read_all_stats`. `oo_target_owner` is a member of `pg_read_all_stats` with inherit true and set false. That membership is the installer grant.
- `information_schema` column and table privilege lists for `probe_orders` contain `oo_r_a`, `oo_r_b`, and `oo_target_owner`. Import-table `relacl` values name only `oo_target_owner`.
- Before `SET LOCAL ROLE oo_r_a`, `set_config('oo.claims', '{"region":["eu"]}', true)` succeeded. After the role change, `set_config` returned `42501`. In the same shape of transaction, `SET LOCAL oo.claims = '{"region":["us"]}'` produced no error, and `current_setting('oo.claims', true)` returned `{"region":["us"]}`.
- `RESET ROLE` returned to `oo_a169_login`, `set_config` then stored `{"region":["us"]}`, and the following `SET LOCAL ROLE oo_r_a` observed that value.

`pg/probe.exit` is 0. `pg/sqlstate.exit` is 0. The redact check found neither password in the probe outputs.

## Cleanup

`cleanup.txt` removes only objects labeled `oo.owner=a169` and this session: containers `oo-a169-pg` and `oo-a169-rust194`, volumes `oo-a169-cargo`, `oo-a169-target`, and `oo-a169-pgdata`, network `oo-a169-net`, and the `/tmp` secret directory. Remaining labeled containers and volumes: 0. No prune.

## Author check

The same session wrote the probe, ran it, and read the outputs. That is not a second reviewer. No release is claimed. No parent ledger checkbox is updated.
