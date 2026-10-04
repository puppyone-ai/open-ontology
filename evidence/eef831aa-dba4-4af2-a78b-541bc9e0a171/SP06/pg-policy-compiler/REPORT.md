# a171 report — column ACL install

Task `a171`. Session `eef831aa-dba4-4af2-a78b-541bc9e0a171`. Repository `/Users/redux/puppy-one/open-ontology`. Fixed base `3dadf678ff0fbfdc1ddd2ac4144ee68cb2ba772c`. Work continued from `d61945f1ae66f9d3b35666385cc9d94d5d4ae08a`. Source commit `d3429726618a4e69ada7fb35131f9043a21e23e9` (`gateway: install a scoped column ACL on a trusted TLS session`).

The gateway library compiles a column ACL and installs it on a trusted administrator TLS session. The live test builds a shadow with the reviewed `_oo.full_import_prepare_shadow` and checks the stored grants. Contract `contracts/gateway/pg-policy-v1.md`.

## Pre-edit smoke

One Linux arm64 container, image `docker.io/library/rust@sha256:0f24958fa693fe838859faf64a0ec78d382cd543a6b6efd0fa7a3bcb10ea90d5`, toolchain `rustc 1.94.0 (4a4ef493e 2026-03-02)`, host `aarch64-unknown-linux-gnu`. This smoke finished before the product edits and was not repeated. `linux/docker-run.exit` is 0. `linux/phases/worst.exit` is 0. `linux/lock-oracle.txt` recorded `lock_has_tokio_postgres=no` at that time. Clippy `-D warnings` on `oo-gateway --lib`, the empty lib test, and the three architecture tests passed.

## Install builder

The same Rust image, one container at a time, name `oo-a171-rust194`. Private bridge `oo-a171-net`, cap drop `ALL`, no new privileges, no host network, no published port, no host home, `HOME=/tmp`, cargo volume `oo-a171-cargo`, target volume `oo-a171-target`. The repository mount was read-write so `cargo` could rename `Cargo.lock`. Each launch recorded `host_cargo=0 rust_builders=0 global=0`.

The green run is `linux/install-docker-run.exit` = 0. Inspect: id `7442751e9e8d6ea459c0a0f46e5777830b64296af01da7f6d6133606a8224cdd`, exit 0, not OOM, not privileged, network `oo-a171-net`.

| Phase | Exit |
| --- | --- |
| clippy component | 0 |
| `cargo fetch --manifest-path Cargo.toml` | 0 |
| lock churn (removed pairs) | 0 |
| `cargo clippy --locked --offline -p oo-gateway --all-targets -- -D warnings` | 0 |
| `gateway_postgres_stack_is_allowed` | 0 |
| `gateway_sync_postgres_and_sqlx_are_forbidden` | 0 |
| `dev_only_gateway_pipeline_is_forbidden` | 0 |
| `cargo test --locked --offline -p oo-gateway --lib --test policy_pg_live -- --test-threads=1` | 0 |
| docker run | 0 |

Lib tests: 5 passed. Live test: 1 passed (`shadow_column_acl_installs_and_denies`, 0.25s). Raw log: `linux/install-docker-run.log`.

Failed attempts are kept and were not overwritten by the green log:

| Attempt | Stop |
| --- | --- |
| `linux/attempt1-clippy` | clippy `-D warnings` on unused `mut` and `prepare_shadow(&Client)` |
| `linux/attempt2-uuid-bind` | adopt parameter was `&str` against PostgreSQL `uuid` |
| `linux/attempt3-role-collision` | `CREATE ROLE` collided with roles left by the previous database |
| `linux/attempt4-numeric-bind` | `_oo_pos` parameter was `&str` against `numeric` |

## Database

One PostgreSQL container, image `docker.io/library/postgres@sha256:3aac91d19d5b7aff4c650dddfacc1a26c436459aafd4eb51ab0bf1748e990dbb`, name `oo-a171-pg`, started once and reused. `pg/up.exit` is 0. `pg/server-san.txt` has DNS `oo-a171-pg`. `pg/ports.json` has no host binding. `pg/caps.json` drops `ALL` and adds `CHOWN`, `DAC_OVERRIDE`, `FOWNER`, `SETGID`, `SETUID`. The container was not privileged. Admin and login passwords and the CA private key lived only under `/tmp/oo-a171-eef831aa-dba4-4af2-a78b-541bc9e0a171/secrets` and were removed in cleanup. A scan of this evidence directory found neither password and no private-key file.

The test drops and recreates database `oo_a171`, drops the previous fixture roles (`oo_target_owner`, `oo_pipeline_runtime`, `oo_a171_login`, and `oo_r_*`), then applies frozen SQL in the installer transaction shape. It does not link `oo-pipeline`. `0002` is not applied. `SET ROLE oo_target_owner` happens only inside that fixture schema install. Policy install does not `SET ROLE`.

File SHA-256, unchanged from the a169 measurement:

| File | SHA-256 |
| --- | --- |
| `0000_bootstrap_roles.sql` | `37d745803589ba48cde1703a4ed7c4c63a956c7c5074aa618851d09db1e26208` |
| `0001_target_fence_expand.sql` | `97a5bad069392988a4ee5f7b58a7bc526885b17aa26c606566aa4c8671fb96e9` |
| `0003_target_segment_preflight_expand.sql` | `d046d98b825dbf45aa5a60c886fc47e7dd1fcbce074a5dace5a75bd0693e15a3` |
| `0004_physical_mapping.sql` | `844de6f645f8f1ebf29d05118ae25d4669d8dc870eed1fa7697d4b98456eaf95` |
| `0005_full_import_expand.sql` | `a9cef2013019784857353ff4fe060350d71a4e9ac4ab18551b992a7a9b52ecb6` |

The passing test required `_oo.schema_migrations` to contain exactly those five versions and `applied_by = oo_target_owner` on all five rows.

## Measured install

Oracle `live/oracle.txt`. The shadow row was `_oo_pos` 1, `order_id` 42, `region` west, `secret` hidden-value. The oracle records outcomes, not the secret value and not either password.

- Physical table `pt_dbd241f47a437a629a0962159a292835f5639d63aff9b9f969cff08d0b17`. Columns `pc_4ec8c9b1…` (`order_id`), `pc_77fc1b09…` (`region`), `pc_d83c68b1…` (`secret`).
- A row filter, unknown PII, a stale generation (policy 3, current 7), and a missing login were refused. After those refusals, no `oo_r_` role had been added. `oo_r_orphan` was absent.
- `oo_r_dispatcher` installed, then replayed with the same physical columns. `oo_r_clerk` installed. Session user and membership grantor were `oo_fixture_admin`. Membership is inherit false, set true, admin false.
- Role flags for `oo_r_dispatcher` are all false: no login, not superuser, not createdb, not createrole, not inherit, not bypassrls, not replication.
- Column ACL display: `order_id` is `oo_r_dispatcher=r/oo_target_owner` and `oo_r_clerk=r/oo_target_owner`. `region` is `oo_r_dispatcher=r/oo_target_owner`. `secret` and `_oo_pos` are null. The table `relacl` is owner rights plus `oo_pipeline_runtime=a/oo_target_owner`. It does not name the reader role or the login.
- `information_schema.table_privileges` count is 0 for `oo_r_dispatcher`, `PUBLIC`, and the login. Public column privileges count is 0.
- Reader TLS was true on `pg_stat_ssl` for backend pid 175. After each `ROLLBACK`, the same pid remained. Dispatcher read returned 42 and west. Clerk read returned 42 for `order_id`.
- SQLSTATE `42501` for dispatcher on `secret`, `_oo_pos`, the column `extra` added after install, `INSERT`, and `UPDATE`; for dispatcher on `_oo.physical_mappings` and `_oo.full_import_run`; for clerk on `region`; and for the login with no `SET ROLE` on `order_id`.

The client uses `SslMode::Require`, a rustls `ClientConfig` with the fixture CA, and server name `oo-a171-pg`. There is no `NoTls` path and no test-only verifier.

## Dependency delta

See `supply/dependency-delta.md`. Twenty-four packages were added. Zero existing versions were removed. Checksums of `tokio` 1.53.1, `rustls` 0.23.45, `tokio-rustls` 0.26.5, `digest` 0.11.3, `whoami` 2.1.3, `sha2` 0.10.9, `sha2` 0.11.0, and `phf` 0.12.1 are unchanged. This run did not execute `cargo deny` or `cargo audit`. The install result is not a release approval.

## Cleanup

`cleanup.txt`. Removed `oo-a171-rust194`, `oo-a171-pg`, volumes `oo-a171-cargo`, `oo-a171-target`, and `oo-a171-pgdata`, network `oo-a171-net`, and the `/tmp` secret directory. Each command exited 0. No other container was removed. `docker prune` was not used.

`SHA256SUMS` lists every other file in this directory. `SHA256SUMS.sha256` is the digest of that list. Source bytes are under `source/blobs/`.
