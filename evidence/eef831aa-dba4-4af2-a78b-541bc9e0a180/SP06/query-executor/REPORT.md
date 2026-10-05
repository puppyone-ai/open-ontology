# a180 permission-bound SELECT executor

UUID `eef831aa-dba4-4af2-a78b-541bc9e0a180`. Slice only. SP06-W04, SP06-W05, and SP06-W06 remain open. This run does not accept the gateway or a release.

Source commit `8a289f0c699abb8c707ca45ad45cb036abc3fc05`.
Parent `5abe6bdcfa75c9b6f70a7f756481bb25ddc193d9`.
Fixed ancestor `03512f91324ac33e772e9702fe806e8e04037ec4` (a175 SQL guard). `git merge-base --is-ancestor` succeeded.
Author `Redux0223`. One session wrote the code and ran the tests. Independent review remains open.

## What landed

Owned paths in the source commit:

- `crates/oo-gateway/src/query_executor.rs`
- `crates/oo-gateway/src/lib.rs` (`pub mod query_executor` and the module sentence)
- `crates/oo-gateway/tests/query_executor_pg_live.rs`
- `contracts/gateway/query-executor-v1.md`

`Cargo.toml` and `Cargo.lock` were not edited. The public guard API was sufficient. `sql_guard`, policy, manifests, migrations, and core were not edited.

The entry is `DedicatedSession::execute_trusted_select`. It calls `guard_select` on the original SQL and runs `GuardedPlan.executed_sql`. The request type has no executed-SQL field. `UNTRUSTED_ADMISSION` is `Blocked`. There is no 500 ms process-isolated parser worker.

## Images and runtime

| Piece | Identity |
|---|---|
| Rust | `docker.io/library/rust@sha256:0f24958fa693fe838859faf64a0ec78d382cd543a6b6efd0fa7a3bcb10ea90d5` |
| Rust platform | `linux/arm64`, `rustc 1.94.0 (4a4ef493e 2026-03-02)` |
| PostgreSQL | `docker.io/library/postgres@sha256:3aac91d19d5b7aff4c650dddfacc1a26c436459aafd4eb51ab0bf1748e990dbb` |
| Server | `17.10 (Debian 17.10-1.pgdg12+1)`, `track_functions=pl` |
| Builder | `oo-a180-rust194`, network `oo-a180-net`, privileged false, `CapDrop=ALL` |
| Database | `oo-a180-pg`, privileged false, no host port |

Private cargo volume `oo-a180-cargo`, target volume `oo-a180-target`. `HOME=/tmp` inside the builder. No host network, no host home, no shared cargo cache, no pull.

## Commands and exits

Pre-edit smoke exits under `linux/phases/` are all `0` (`smoke-fetch`, `smoke-test`, `smoke-clippy`, `fetch`, `test`, `clippy`, `clippy-component`, `libclang`, `libclang-load`).

Green run under `linux/live/`:

| Phase | Exit |
|---|---|
| fetch | 0 |
| lib-test | 0 |
| clippy-component | 0 |
| clippy | 0 |
| pg-test | 0 |
| docker-run | 0 |
| launch | 0 |

Lib tests: `cargo test --manifest-path Cargo.toml --locked --offline -p oo-gateway --lib`. 10 passed, 0 failed. Five of those are `query_executor` unit tests.

Clippy: `cargo clippy --manifest-path Cargo.toml --locked --offline -p oo-gateway --lib --test query_executor_pg_live -- -D warnings`. Finished clean. `tests/sql_guard.rs` is outside this gate.

Live: `OO_A180_LIVE=1 cargo test --manifest-path Cargo.toml --locked --offline -p oo-gateway --test query_executor_pg_live -- --nocapture`. `live_permission_bound_select` passed in 5.65 s.

Host capacity lines on the launches were `host_cargo=0 rust_builders=0 global=0 cap=3`. This task used one builder and one PostgreSQL.

## Live oracle

`live/oracle.txt`. Passwords are not in that file. The comparison finished with `live_result=compared`.

- Star `SELECT` returned 4 rows, status succeeded, rollback confirmed by the summary assertions in the test.
- Row bound: 2 rows, `fixture.note_read` calls 3, truncated.
- Byte cap: 1 row, 4 bytes, truncated. Oversized row: 0 rows, truncated, succeeded.
- Role reuse refused. Hidden column, unmapped `extra`, and `INSERT` refused by the guard before audit.
- Direct privileges: secret, extra, and role B on the hidden int2 returned `42501`. Role B on int4 returned `ok:4`.
- Type refusal OID 1700, call delta 0. Lying `extra` returned SQLSTATE `42501` and failed. Dropped stream status `partial`, `end_audit_ok` false. Audit admit failure ran no query.
- Deadline SQLSTATE `57014`, elapsed 1786 ms. In-flight cancel `sent` with `57014`. Late cancel `not_sent` then `sent`.
- Role B star: 4 rows, 1 column.
- Audit rows remained after the query session closed (`audit_durable=4`). The column role has no `INSERT` on `fixture.audit_log`.

Failed attempts 1 through 8 are kept under `linux/live-attempt-*`. They are the compile and proof misses that preceded this green run. FINDINGS lists them.

## Bounds kept

Whole-query wall clock is one client `Instant`, maximum 30 s. Portal page size is 1. `max_rows + 1` is the fetch ceiling. `max_result_bytes` is checked before append. Result OIDs are bool 16, int8 20, int2 21, int4 23, text 25, and null. The dedicated client is not pooled. Failure drops the socket. Cancel uses the bound `backend_pid` and `backend_start` plus a generation fence.
