# a162 import-pg command

Author run on commit `3fba40dd9fdebaa219ab4eb7fdb39b8fcf1a1e4a`, tree `82b5549ca70475ba6b59eb63879d4e8f0719617a`, parent `d1b4e86c6b52c30337fbb5281e934b6bc82419e7`. The fixed base `cc764cbc5f9d5d2cf6056a2c41ff2612161b5ff4` is an ancestor. Full import stays the reviewed entry from `476d21c7`. This file is the author record. Independent review is next.

`oo-pipeline import-pg` reads one `oo.pipeline.import-pg/v1` document from stdin and takes no other arguments. It calls `PgConnector::prepare_snapshot`, then `import_unpublished` on one runtime connection. Stdout is the `RunSummary` that call returns, protocol `oo.pipeline.import-pg-result/v1`. The `_oo.full_import_run` row is the seal. `awaiting_publish` is not publication. The command does not install a schema and does not take bootstrap credentials.

## Source

| Item | Value |
| --- | --- |
| Commit | `3fba40dd9fdebaa219ab4eb7fdb39b8fcf1a1e4a` |
| Tree | `82b5549ca70475ba6b59eb63879d4e8f0719617a` |
| Parent | `d1b4e86c6b52c30337fbb5281e934b6bc82419e7` |
| Subject | pipeline: add an import-pg command |
| Product paths | 11 files, 1981 insertions, 9 deletions |
| Lock change | `serde` and `serde_json` names on the `oo-pipeline` package only. Versions stayed `1.0.229` and `1.0.151` |
| Root `Cargo.toml` | unchanged |
| Frozen SQL, migrations, installer, `full_import.rs`, encoder, mapping | unchanged |

Blob ids and byte copies are in `SOURCE.txt` and `source/blobs/`. The copies match the commit and the worktree (`mismatches 0`).

## Package gate

One `linux/arm64` Rust builder, image `docker.io/library/rust@sha256:0f24958fa693fe838859faf64a0ec78d382cd543a6b6efd0fa7a3bcb10ea90d5`. Raw inspect is `images/rust.json`. `rustc 1.94.0 (4a4ef493e 2026-03-02)`, host `aarch64-unknown-linux-gnu`. Container `oo-a162-live6` was removed after exit 0. Private network `oo-a162-eef831aa-net`, `cap-drop ALL`, not privileged, no published ports, `HOME=/tmp`, private cargo and target volumes. Host config: `linux/live-6/hostconfig.txt`.

| Phase | Exit | Result |
| --- | --- | --- |
| rustup clippy | 0 | component present for this container |
| `cargo test -p oo-connectors --lib --locked --offline` | 0 | 64 passed |
| `cargo clippy -p oo-connectors --all-targets --locked --offline -- -D warnings` | 0 | `Finished dev` in 1.09s |
| `cargo test -p oo-pipeline --lib --locked --offline` | 0 | 11 passed |
| `cargo clippy -p oo-pipeline --all-targets --locked --offline -- -D warnings` | 0 | `Finished dev` in 0.59s |
| `cargo test -p oo-pipeline --test import_pg_cli_live` | 0 | 3 passed, 1 ignored |
| prepare live | 0 | 1 passed in 1.60s |
| import-pg binary live | 0 | 1 passed in 2.08s, 3 filtered |

`sqlx-postgres` printed two unfulfilled `dead_code` expectations under `crates/oo-connectors/vendor/sqlx-postgres`. Those are dependency warnings. The crate under `-D warnings` still exited 0. Vendor was not edited. Logs: `linux/live-6/`.

The pre-edit smoke is `linux/smoke-before/` (inside exit 0). Its first container lacked the clippy component (`linux/smoke-before/attempt-1/`, inside exit 1). `linux/build/` and `linux/build-2/` are compile failures, inside exit 101. `linux/build-3/` is a green compile gate from before the session rollback change. The ignored tests were not run there.

## Private PostgreSQL

One server, image `docker.io/library/postgres@sha256:3aac91d19d5b7aff4c650dddfacc1a26c436459aafd4eb51ab0bf1748e990dbb`. Raw inspect is `images/postgres.json`. `postgres (PostgreSQL) 17.10 (Debian 17.10-1.pgdg12+1)`. Container `oo-a162-pg` `b78e5bb4c3b15ef73221c8b7279ed622895d54e6c5e6c1c5925308a85451f758`. Network alias `oo-a162-pg`, no published ports, not privileged, memory 1g. Source and target were databases on this server. The Rust builder mounted `ca.crt` only. The server key stayed on the postgres mount. CA fingerprint is `pg/ca-fingerprint.txt`. Passwords were files, not process arguments.

## Public factory

`linux/live-6/prepare.stdout`: `prepare_snapshot_reads_the_public_session` passed. The connector logged in as `oo_a162_prep_reader`. Admin SQL was fixture setup and the comparison oracle.

The prepared request named that reader, database `oo_a162_prep`, and the admin oracle's recovery flag and `pg_postmaster_start_time()`. Admitted `app.item` columns and the primary key `id` matched `pg_attribute`. `Debug` of the request contained neither the reader password nor `BEGIN CERTIFICATE`. `SnapshotOwner::open` on that request succeeded, the binding repeated the reader, database, and postmaster, and `abort` returned. `pg_stat_activity` for `application_name = oo-pg-source` returned to 0.

A writer login with `INSERT` on `app.item` was refused with fact `registration_refused`. After a second prepare at generation 2, `ALTER TABLE app.item ADD COLUMN extra int4`, and `SnapshotOwner::open` at generation 3, the owner returned fact `catalog_identity`.

## Binary import

`linux/live-6/import.stdout`: `import_pg_binary_on_private_postgres` passed in 2.08s. The process was `oo-pipeline import-pg` with the request on stdin. Two selected tables: `app.crates` (`spruce-keel`, `oak-rib`) and `app.tags` (`hull`, `keel`, `deck`).

The missing-readiness path installed only the pre-`0005` schema, granted `CONNECT`, and ran the binary. Exit 1. Stdout empty. Stderr `oo-pipeline: target schema is not ready`. `to_regclass('_oo.full_import_run')` stayed null. Source `application_name = oo-pg-source` count was 0. Files: `linux/live-6/out/not-ready.exit`, `not-ready.stderr`, `not-ready.stdout`.

The success path then installed `0005` with the existing installer API and granted `oo_pipeline_runtime` to the runtime login. The binary exited 0. Stderr empty. Stdout is `linux/live-6/out/success.stdout`:

`run_id` `48117413-5708-4129-8b50-51e11d7675cd`, `state` `awaiting_publish`, `source_generation` 7, `target_generation` 1, `table_count` 2.

| Table | Rows | Canonical digest | Physical name |
| --- | --- | --- | --- |
| `app.crates` | 2 | `07ba797b71a38e7b3e2dc485331078c5f604a9337f75d111caafc21c0f577d66` | `pt_e478927e10f4cba30eae037d976953f8b72f8dd37b4e651b382df75fed8c` |
| `app.tags` | 3 | `b44c8c88ea14f34c0f9cb82b9efbf1d34ead1bd237b0299f80f91eb51ffc21a4` | `pt_ce253c3b039995f4103d0dfb274b3eb3d8cc511107436f55e7feb3d333c3` |

A new admin connection read `_oo.full_import_run` for that `run_id`: state `awaiting_publish`, `sealed_at` not null. Stored digests and applied rows matched. `_oo.physical_mappings` matched the physical names. Shadow values matched the source rows, ordered by `_oo_pos`. `pg_publication` count was 0. The agent `SELECT` on the crates shadow failed with SQLSTATE `42501`. `app.write_audit` for `oo_a162_reader` stayed 0. A writer `INSERT` raised that count for `oo_a162_writer` to 1 and left the reader at 0. Outcome file: `linux/live-6/out/outcome.txt` (`import_pg_cli_live_ok`).

The non-ignored process tests in the same binary cover usage exit 2, invalid JSON, a duplicate key, an unknown field, a bad protocol, a missing file, a directory, a symlink, a zero row budget, deadlines 0 and 120001, generation 0, and a generation above `i64::MAX`. Stdout stayed empty. The sentinel `s3cret-a162-marker` stayed out of stdout and stderr. Those tests passed in `linux/live-6/cli-errors.stdout` (3 passed, 1 ignored).

## What this run leaves in place

Publication stayed at 0. The agent was denied. The type policy on the request was `preserve_row`. The a159 fault campaign was not repeated. Cancel and source-failure behavior remain in `import_unpublished`. `full_import.rs` was not edited. `RunSummary` is still publicly constructible. The contract and the `import_cli` module docs say stdout is the returned summary and the database row is the seal.
