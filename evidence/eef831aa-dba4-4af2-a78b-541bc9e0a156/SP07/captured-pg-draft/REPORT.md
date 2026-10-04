# a156 captured PostgreSQL draft

Session `eef831aa-dba4-4af2-a78b-541bc9e0a156`. Source commit `63466471a6d403e0a2a29c8d1ae064179da65bd8`. Tree `1a6c65addb3897fe93159560b8d295949eb2542c`. Parent `377707e88b13b2b77baa6f619c2c1b364f0dab28`. Fixed base `e6bd4c7aa8c3a43cc0516a21e325765c5eecb2f6`. The builder tree at that parent still matches the fixed base.

`compile_captured_pg_catalog` turns one `oo.source.pgsql/v1` catalog document into an evidence envelope and a structure draft. The builder does not open a socket and does not depend on `oo-connectors`. Protocol `oo.builder.captured-pg-draft/v1`. Compiler `oo-builder/captured-pg-draft/v1`.

## Source blobs

| Path | Blob |
|---|---|
| `contracts/builder/captured-pg-draft-v1.md` | `916fe4c053b91d072fbc16fe000e7b41206d940c` |
| `crates/oo-builder/src/captured_pg.rs` | `373bcad7968d88d9be331e4622b63d1ca60b1449` |
| `crates/oo-builder/src/draft.rs` | `9505a8fd63e5c2517622c695d9ffeda4a639b562` |
| `crates/oo-builder/src/lib.rs` | `5849b4884e08ce2bdb8a553d56733748ef1715cc` |
| `crates/oo-builder/src/pgsql.rs` | `c7f399aa919c18858ae9d29da22deabd01ebe115` |
| `crates/oo-builder/tests/captured_pg.rs` | `e2218b0dc82ecaec39f67dd00a580cd5468e2f7c` |

No `Cargo.toml`, `Cargo.lock`, connector, pipeline, spec, or ledger file is in that commit.

## Linux 1.94

Image `docker.io/library/rust@sha256:0f24958fa693fe838859faf64a0ec78d382cd543a6b6efd0fa7a3bcb10ea90d5`. `rustc 1.94.0 (4a4ef493e 2026-03-02)`, host `aarch64-unknown-linux-gnu`.

Pre-edit smoke, current `linux/smoke/`, docker exit 0. Library tests: 27 passed, including the checked-in ontology-draft golden. `linux/smoke/attempt-1/` is the earlier run that lacked `examples/ontology-draft` and stopped at 26 passed.

Focused check on the fixed-base archive plus this overlay, docker exit 0:

| Phase | Exit | Result |
|---|---|---|
| `cargo test -p oo-builder --lib` | 0 | 28 passed |
| `cargo test -p oo-builder --test captured_pg` | 0 | 6 passed |
| `cargo test -p oo-builder --test structure_draft_cli` | 0 | 4 passed |
| `cargo clippy -p oo-builder --lib --test captured_pg -- -D warnings` | 0 | clean |

Rustfmt 1.94 checked `pgsql.rs`, `captured_pg.rs`, and `tests/captured_pg.rs`. `draft.rs` kept its previous layout. Earlier focused failures are under `linux/focused/attempt-1` through `attempt-4`.

## Live catalog

PostgreSQL image `sha256:3aac91d19d5b7aff4c650dddfacc1a26c436459aafd4eb51ab0bf1748e990dbb`, server `17.10 (Debian 17.10-1.pgdg12+1)`, private network alias `oo-a156-pg`, database `oo_a156`. No host port. Reader `oo_a156_reader` has `SELECT` only. Tables are owned by `a156_owner`.

The probe called `PgConnector::new`, `connect`, and `PgSession::catalog`, then `serde_json::to_string`, then `compile_captured_pg_catalog`. Capture and the post-alter compile both exited 0.

| Check | Result |
|---|---|
| Selected columns | `shop.customer` `tenant_id`, `id`, `email`; `shop.purchase` `tenant_id`, `id`, `customer_id` |
| Composite primary key | `tenant_id`, `id` on both tables |
| `purchase_customer_fk` | `many_to_one`, `declared_fk`, status `draft`, confidence null, join hint `left` |
| Email | `text`, collation oid 100 name `default`, PII and semantic type pending |
| Comment | byte length 35, sha256 `e2b8d8558b447b6d8a87baa45536610bfc282c42f1c7418b11ccc6068da183a7`, body absent from catalog, draft, and wrapper |
| Reorder | reversed tables, columns, and privileges produced the same draft, envelope, and wrapper |
| Schema change | `ALTER TABLE shop.customer ADD COLUMN note text` changed schema hash `298397578839b738be31fe7f0a36107c3b552664a5b0ca8ce2719e2a3be06978` to `488bf2d3b0e833c0a5fde9e86e3f547acd969c5fd365c58e3cdefdceef725685` |
| Prior decision | `stale_decision` detail `stale_schema_hash`, state `pending` |
| Curated label | echo keeps `Email address`; effective label stays `email`; PII stays unapplied |
| Wrapper | `live_catalog_review=pending`, `row_import=not_proven`, `grants=not_authorized` |
| Legacy draft | `pg_catalog_integration=not_accepted`, commit `b2d3cbc5af2b97d6565cbd67f0a9208f778a8c74` |

Probe lock diff adds only package `a156-captured-pg-probe` 0.1.0. Resolved versions stay `sqlx` 0.9.0, `tokio` 1.53.1, `serde_json` 1.0.151, `sha2` 0.10.9. The probe is not a workspace member of the product repo.

## Reader SQL

`log_statement=all` was on before either reader session. Corrected audit of the same log: 34 reader statements, 0 writes. Keywords: `SELECT` 30, `START` 2, `ROLLBACK` 2. The admin session that added `note` accounts for the one `ALTER`. The first classifier exited 1 because it ignored extended-protocol `execute` lines and treated `START TRANSACTION` as unclassified. That output is `pg/audit-attempt-1.txt`.

This capture does not prove a row import, a grant, or publication. SP07 is not marked complete.
