# a164 import-pg command review

Verdict: **APPROVE LIMITED** `oo-pipeline import-pg` and `PgConnector::prepare_snapshot` on commit `3fba40dd9fdebaa219ab4eb7fdb39b8fcf1a1e4a`, tree `82b5549ca70475ba6b59eb63879d4e8f0719617a`, parent `d1b4e86c6b52c30337fbb5281e934b6bc82419e7`.

The approved surface is one stdin document `oo.pipeline.import-pg/v1`, a read-only `prepare_snapshot`, and one call to the already reviewed `import_unpublished`. Stdout is the `RunSummary` that call returned. The `_oo.full_import_run` row is the seal. `awaiting_publish` is not publication.

This approval does not cover publication, an Agent grant, a full type certificate, CDC, a control-plane lifecycle, a customer, a release, or parent SP04. The unpublished loader campaign stays the a159 review `476d21c7`.

## Fixed source

| Item | Value |
| --- | --- |
| Commit | `3fba40dd9fdebaa219ab4eb7fdb39b8fcf1a1e4a` |
| Tree | `82b5549ca70475ba6b59eb63879d4e8f0719617a` |
| Parent | `d1b4e86c6b52c30337fbb5281e934b6bc82419e7` |
| Subject | pipeline: add an import-pg command |
| Author evidence | `fe7e4741` (not this review's proof) |
| Loader proof | `476d21c7` on `cc764cbc5f9d5d2cf6056a2c41ff2612161b5ff4` |
| `full_import.rs` blob | `dfaf2470419dc5e07fbe92ae60b099ab96ab788c`, same object as `cc764cbc` |
| `0005` SHA-256 | `a9cef2013019784857353ff4fe060350d71a4e9ac4ab18551b992a7a9b52ecb6` |
| serde | `1.0.229` |
| serde_json | `1.0.151` |

The workspace was not the review input. `HEAD` at delivery was `cf9bdb98e43d7702a81ff1214a678a8a69363b1e`. The smoke and the probe compiled `git archive` of `3fba40dd` only. `git diff cc764cbc 3fba40dd` is empty for `full_import.rs`, the encoder, the sink, physical mapping, and `migrations/`. `Cargo.lock` adds the names `serde` and `serde_json` on the `oo-pipeline` package. Those versions were already in the lock. Root `Cargo.toml` is unchanged.

## What ran

Pre-edit smoke, `linux/smoke/`, image `docker.io/library/rust@sha256:0f24958fa693fe838859faf64a0ec78d382cd543a6b6efd0fa7a3bcb10ea90d5`, `rustc 1.94.0` host `aarch64-unknown-linux-gnu`. No overlay and no PostgreSQL. Exits are 0: fetch, `oo-connectors` lib (64), `oo-pipeline` lib (11), `import_pg_cli_live` without `--ignored` (3 passed, 1 ignored), and Clippy `-D warnings` for both packages. Archive hashes matched the commit. Changed sources have `forbid(unsafe_code)` and no `unsafe` token.

Deciding PostgreSQL is `docker.io/library/postgres@sha256:3aac91d19d5b7aff4c650dddfacc1a26c436459aafd4eb51ab0bf1748e990dbb`, `17.10 (Debian 17.10-1.pgdg12+1)`, TLS verify-full, container `oo-a164-pg`. One builder at a time. `linux/probe/` and `linux/probe-2/` are preserved failures of the review harness (a lifetime, then an audit count that included the fixture owner). `linux/probe-3/` is the green combined run. Its `out/success.stdout` was then overwritten by the existing author test, which writes the same filenames. `linux/probe-4/out/a164-success.stdout` is the independent oracle.

## Fresh fixture

`yard.hulls` rows `(1, cedar-stem, 420)` and `(2, ash-stem, 380)`. `yard.fasteners` rows `(10, copper-rove)`, `(11, bronze-nail)`, `(12, iron-rivet)`. The reader has `SELECT` only. The writer has `INSERT` on `yard.hulls` and is refused.

| Claim | Result | Evidence |
| --- | --- | --- |
| Public `prepare_snapshot` and public `inspect_access` then `catalog` | reader registered; facts match `oo_a164_reader`, database `oo_a164_src`, recovery false; column attnum, name, type oid, and primary key match `pg_attribute` | `linux/probe-4/out/a164-outcome.txt` `facts_and_catalog=bound` |
| Writer registration | fact `registration_refused`; reader and writer audit rows stayed 0 | `writer_registration=refused` |
| Changed catalog | `SnapshotOwner::open` after `ALTER TABLE ... ADD COLUMN` returns `catalog_identity` | `changed_catalog=refused` |
| Target not ready | binary exit 1, stdout empty, stderr `oo-pipeline: target schema is not ready`, `_oo.full_import_run` absent, `full-import/v1` migration count 0 | `a164-not-ready.exit` `1` |
| Two-table import | binary exit 0, stderr empty, rows 2 and 3, shadow text equals the source | `a164-success.stdout` |
| Canonical digests | stdout, `_oo.full_import_table`, and a second `SnapshotOwner` stream agree | hulls `781b94a77f0795fb3597c58a03c4d70be754ae5a5833ab22471e2f0732fbc175`; fasteners `57bdcde8f8e66cc23ee6ee5281c409fe61ef68945983e4bdbc0e713aa13c08af` |
| Seal | run `48ffd752-5224-4bca-ac1b-3525237b624e`, state `awaiting_publish`, `sealed_at` set, `retired_at` null, `pg_publication` count 0 | `awaiting_publish_row=authoritative` |
| Agent | `SELECT` on `_oo.full_import_run` returns SQLSTATE `42501` | `agent_denied=42501` |
| Source writes | reader audit 0; a later writer `INSERT` raises the writer audit to 1 | `source_writes=reader_0_writer_1` |
| Constructed summary | `RunSummary { state: AwaitingPublish, run_id: 00000000-0000-4000-8000-000000000164, .. }` compiles and is not a stored row | `constructed_run_summary_rows=0` |
| Secrets | passwords and `PRIVATE KEY` absent from the captured stdout and stderr | `reject_secrets` in the passing test |

Rollback depth, same server, `linux/probe-3/out/depth-outcome.txt`: after `inspect_access` and after `catalog`, driver depth is 0. An explicit begin, rollback, begin cycle is depth 1, 0, 1. Depth 0 with the in-transaction flag set poisons with fact `rollback` and sends no SQL. Depth 2 poisons, leaves depth at 2, leaves the backend `idle in transaction`, and the socket drop removes that backend.

Existing tests on the same server in `linux/probe-3/`: `prepare_snapshot_reads_the_public_session` passed, `import_pg_binary_on_private_postgres` passed, and the three non-ignored command tests passed (usage exit 2, invalid input, secret-free static errors). The a159 fault matrix was not repeated.

## Dependency and safety

Lock metadata only. `linux/smoke/serde-pins.txt` records `serde 1.0.229` and `serde_json 1.0.151`. Clippy stderr still reports two unfulfilled `#[expect(dead_code)]` annotations in `crates/oo-connectors/vendor/sqlx-postgres`. The package commands exited 0. Those files were not part of this commit. The overlay is test-only and was not committed to the product tree.
