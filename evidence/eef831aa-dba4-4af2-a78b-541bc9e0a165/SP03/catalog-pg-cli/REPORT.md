# a165 catalog-pg command

Author run on commit `34498e75d5876d74ddb5343b1b0f5b9b1adaf1df`, tree `a99506b163e7b0dfcbdcbc4d8236c69546593a94`, parent `a173c58a9f90a10b7853ddc1c3fcb74d1b0befa1`. The fixed base `3fba40dd9fdebaa219ab4eb7fdb39b8fcf1a1e4a` is an ancestor. The pipeline tree at the parent still matched that base. This file is the author record. Independent review is next.

`oo-pipeline catalog-pg` reads one `oo.pipeline.catalog-pg/v1` document from stdin and takes no other arguments. It opens the configured source with `PgConnector::connect`, requires `inspect_access` to report `registration_allowed`, and writes `serde_json::to_vec` of the `SourceCatalog` returned by `PgSession::catalog`. The protocol on stdout is `oo.source.pgsql/v1`. There is no trailing newline and no command envelope. The session is dropped before stdout is written.

## Source

| Item | Value |
| --- | --- |
| Commit | `34498e75d5876d74ddb5343b1b0f5b9b1adaf1df` |
| Tree | `a99506b163e7b0dfcbdcbc4d8236c69546593a94` |
| Parent | `a173c58a9f90a10b7853ddc1c3fcb74d1b0befa1` |
| Subject | pipeline: add a catalog-pg command |
| Product paths | 6 files, 1070 insertions, 15 deletions |
| Lock, manifests, connector, builder | unchanged |
| Fixed base | `3fba40dd9fdebaa219ab4eb7fdb39b8fcf1a1e4a` |

Blob ids and byte copies are in `SOURCE.txt` and `source/blobs/`. The copies match the commit and the worktree (`mismatches 0`).

The parent commit is the a164 evidence record. It did not change `crates/oo-pipeline`.

## Package gate

One `linux/arm64` Rust builder at a time, image `docker.io/library/rust@sha256:0f24958fa693fe838859faf64a0ec78d382cd543a6b6efd0fa7a3bcb10ea90d5`. Raw inspect is `images/rust.json`. `rustc 1.94.0 (4a4ef493e 2026-03-02)`, host `aarch64-unknown-linux-gnu`. The green container `oo-a165-post-3` (`b7eedd50c62df62d7842ae338d5b22a3b89bd4d8e29e126a0d95d8d3eb890aa9`) was removed after exit 0. Private network `oo-a165-eef831aa-net`, `cap-drop ALL`, not privileged, no published ports, `HOME=/tmp`, private cargo and target volumes. Host config: `linux/post-3/hostconfig.txt`.

| Phase | Exit | Result |
| --- | --- | --- |
| rustup clippy | 0 | component added in this container |
| `cargo build -p oo-pipeline -p oo-builder --bins` | 0 | both binaries, one invocation |
| `cargo test -p oo-pipeline --lib` | 0 | 11 passed |
| `cargo test -p oo-pipeline --test import_pg_cli_live` | 0 | 3 passed, 1 ignored |
| `cargo test -p oo-pipeline --test catalog_pg_cli_live` | 0 | 2 passed, 1 ignored |
| `cargo clippy -p oo-pipeline --all-targets -- -D warnings` | 0 | `Finished dev` in 0.57s |
| `cargo test -p oo-builder --test captured_pg_cli` | 0 | 3 passed |
| `cargo test -p oo-builder --test review_pg_cli` | 0 | 4 passed |
| catalog-pg binary live | 0 | 1 passed in 1.02s |

`sqlx-postgres` printed two unfulfilled `dead_code` expectations. The crate under `-D warnings` still exited 0. Vendor was not edited. Logs: `linux/post-3/`.

The pre-edit smoke is `linux/smoke-before/` (inside exit 0, container `f42f360de41f96d4f2c8822f03a7be024e2e2f793c94b7737c935a3f3b43760a`). `linux/post/` and `linux/post-2/` are compile failures, inside exit 101. The ignored test was not run there.

## Private PostgreSQL

One server, image `docker.io/library/postgres@sha256:3aac91d19d5b7aff4c650dddfacc1a26c436459aafd4eb51ab0bf1748e990dbb`. Raw inspect is `images/postgres.json`. `postgres (PostgreSQL) 17.10 (Debian 17.10-1.pgdg12+1)`. Container `oo-a165-pg` `eee98d8a88bfafca4b8efb602677229c875710890fe23e9264694e9ac21b0687`. Network alias `oo-a165-pg`, no published ports, not privileged, memory 1g. The database is source-only. The Rust builder mounted `ca.crt` only. The server key stayed on the postgres mount. CA fingerprint is `pg/ca-fingerprint.txt`. Passwords were files, not process arguments.

## Binary catalog

`linux/post-3/catalog-live.stdout`: `catalog_pg_binary_on_private_postgres` passed in 1.02s. The process was `oo-pipeline catalog-pg` with the request on stdin. Selected tables: `app.customer` and `app.order_row`.

Reader exit 0. Stderr empty. Stdout is `linux/post-3/out/reader.stdout` (4757 bytes, no trailing newline).

| Fact | Value |
| --- | --- |
| protocol | `oo.source.pgsql/v1` |
| source_id | `a165src` |
| database | `oo_a165_src` |
| schema_hash | `30ddee6dfef5c60f3d30ae251048b99d673b9ccfce37631685c08ce153fb215f` |
| `app.customer` | `id` int4 oid 23 primary key; `email` text oid 25 not null |
| `app.order_row` | `id` int4 oid 23 primary key; `customer_id` int4 oid 23 not null |
| foreign key | `order_customer_fk` `customer_id` to `app.customer.id` |
| reader privileges | `select` true, `insert` false |

`linux/post-3/out/oracle.json` is the admin comparison. Column attnum, name, type oid, type name, nullability, primary-key order, and the foreign key matched. Comment hashes matched the fixture texts: table comment length 32 `6dead10162c2c2e584fbd68a662cc76bd0a31d506bec1e1a28947e13bffdf1d7`, email comment length 28 `9d0c7d88d799c7e9e26bd9c4beee59eed53503a6416799daa035518e1f782155`, `customer_id` comment length 25 `fcf65baa608941d22db68c25f7f5c185edf92aa43c0160d3778dca1e4233cc67`. The fixture strings are in the test source so a reviewer can recompute those hashes. They are not in the process output.

Writer exit 1. Stdout empty. Stderr `oo-pipeline: registration refused` (`linux/post-3/out/writer.stderr`). The test then showed that principal can insert, and the reader insert failed with SQLSTATE `42501`. Outcome file: `linux/post-3/out/outcome.txt` (`catalog_pg_cli_live_ok`).

## Draft composition

The same container had built `oo-builder`. The test piped the catalog stdout into `captured-pg-draft` twice and into `review-pg-draft` as `{"catalog": <object>}`. All three stdout files match (`linux/post-3/out/draft.stdout`, 18764 bytes). Stderr is empty. Exit 0.

The draft wrapper records `row_import` `not_proven` and `publication` `unavailable`. Generated columns have `pii` null, with four pending `pii_unknown` items. The foreign key is one `declared_fk` relationship with null confidence and null review, plus one pending `candidate_relationship` item whose detail is `join_hint_left_unverified`.

The non-ignored catalog tests cover usage exit 2, malformed JSON, non-UTF-8, a duplicate key, a bad protocol, an unknown `target` field, and a missing password file. The sentinel `s3cret-a165-marker` stayed out of stdout and stderr. Those tests passed in `linux/post-3/catalog-cli.stdout` (2 passed, 1 ignored). Import usage stayed `oo-pipeline: usage`.

## What this run leaves in place

The catalog command does not open a target, install a schema, import rows, grant, certify, or publish. The builder's draft still says publication is unavailable. `full_import.rs` was not edited. The import command's usage line and its non-ignored tests still pass.
