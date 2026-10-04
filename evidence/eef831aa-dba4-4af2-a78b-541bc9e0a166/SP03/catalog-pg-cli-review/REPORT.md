# a166 catalog-pg command review

Verdict: **APPROVE LIMITED** `oo-pipeline catalog-pg` on commit `34498e75d5876d74ddb5343b1b0f5b9b1adaf1df`, tree `a99506b163e7b0dfcbdcbc4d8236c69546593a94`, parent `a173c58a9f90a10b7853ddc1c3fcb74d1b0befa1`.

The approved surface is one stdin document `oo.pipeline.catalog-pg/v1`, a read-only TLS session, `inspect_access` then `PgSession::catalog` when `registration_allowed` is true, and stdout as `serde_json::to_vec` of that `SourceCatalog`. The protocol is `oo.source.pgsql/v1`. There is no trailing newline. The same bytes are a valid stdin document for the already accepted `oo-builder captured-pg-draft` and `review-pg-draft` commands.

This approval does not cover parent SP03, parent SP07, a full policy, publication, a grant, row import, or a PII decision. Author evidence `ce06598c` is not this proof. The import command remains the a164 review on `3fba40dd9fdebaa219ab4eb7fdb39b8fcf1a1e4a`.

## Fixed source

| Item | Value |
| --- | --- |
| Commit | `34498e75d5876d74ddb5343b1b0f5b9b1adaf1df` |
| Tree | `a99506b163e7b0dfcbdcbc4d8236c69546593a94` |
| Parent | `a173c58a9f90a10b7853ddc1c3fcb74d1b0befa1` |
| Subject | pipeline: add a catalog-pg command |
| Author evidence | `ce06598c` |
| Product diff | 6 files, 1070 insertions, 15 deletions |
| `Cargo.lock` blob | `8f8e74ef761a9f29b35d94ef25625bd0eea2d465`, same object as the parent |
| `oo-builder` binary blob | `c2f1fe4a4ba11eb36263b4eeb20e17fd9a93c09d`, same object as the parent |
| `full_import.rs` blob | `dfaf2470419dc5e07fbe92ae60b099ab96ab788c`, same object as the parent |
| Workspace HEAD at archive | `688401ee3dca600835ff001e4ded8f6a666bab9e` |

The workspace was not the review input. Smoke and the probes compiled `git archive` of `34498e75`. `git diff` of `Cargo.toml`, `Cargo.lock`, and the crate manifests between the parent and this commit is empty. Direct `oo-pipeline` edges in the lock stay `oo-connectors`, `oo-core`, `oo-jobs`, `serde`, `serde_json`, `sha2 0.10.9`, `sqlx`, `sqlx-core`, and `tokio`.

## What ran

Pre-overlay smoke, `linux/smoke/`, image `docker.io/library/rust@sha256:0f24958fa693fe838859faf64a0ec78d382cd543a6b6efd0fa7a3bcb10ea90d5`, `rustc 1.94.0` host `aarch64-unknown-linux-gnu`. No overlay and no PostgreSQL. Exits are 0: fetch, `oo-pipeline` lib (11), `import_pg_cli_live` without `--ignored` (3 passed, 1 ignored), `catalog_pg_cli_live` without `--ignored` (2 passed, 1 ignored), and Clippy `-D warnings`. Archive hashes matched the commit. The changed pipeline sources have `forbid(unsafe_code)` and no `unsafe` token. Container `oo-a166-smoke` `97753425ae302cafb421404a1bd1193c8d3c435c44bda85d06b783c876e7b823`.

Deciding PostgreSQL is `docker.io/library/postgres@sha256:3aac91d19d5b7aff4c650dddfacc1a26c436459aafd4eb51ab0bf1748e990dbb`, `17.10 (Debian 17.10-1.pgdg12+1)`, TLS verify-full, container `oo-a166-pg` `93c0ea346140222fb7df322744957c0ed71fdcfa4ec9b50c02512449de74944d`. The database is source-only. The builder mounted `ca.crt` only. CA fingerprint is `pg/ca-fingerprint.txt`. One builder at a time.

`linux/probe/` failed to compile the first review test (`BTreeMap` entry type). `linux/probe-2/` compiled, ran the live test, and failed Clippy on a derivable `Default`. Those logs are kept. `linux/probe-3/` is the green run. Container `oo-a166-probe-3` `c18d6dcb0df7a4924990e5ee8462192b72a406bf57aac0f38e1cbcb45a58200e`.

## Fresh fixture

`pier.berth` columns `id`, `label`, `note`. `pier.mooring` columns `id`, `berth_id`, foreign key `mooring_berth_fk` to `pier.berth.id`. The reader has `SELECT` only. The writer has `INSERT` on `pier.mooring` and is refused. Comment bodies stay in the overlay source so the hashes can be recomputed. They are absent from process stdout, stderr, and `oracle.json`.

| Claim | Result | Evidence |
| --- | --- | --- |
| Reader process | exit 0, stderr empty, stdout 5244 bytes, no trailing newline, protocol `oo.source.pgsql/v1` | `linux/probe-3/out/reader.exit`, `reader.stdout` |
| Columns, primary keys, foreign key | admin `pg_attribute`, `pg_index`, and `pg_constraint` match attnum, name, type oid, type name, nullability, primary-key order, and `mooring_berth_fk` | `outcome.txt` `columns_pk_fk_comment_hash=match` |
| Comment hashes | table `berth` length 28 `2901b761da219ea1f9881d9e9328ca74fc80a876743d05c35d94081f98cdc446`; `label` length 28 `b9589d60ed95d53fda035dc404d21dcf1268cc4027aa674b4c703bd144b6ec3e`; `berth_id` length 27 `3afbc85a5c8693d35b283d807c9758ce15c92d5b48dd0171893370f5bc246174`; `note` and the `mooring` table comment are null | `oracle.json` |
| Schema hash | `3528d0499acfff0756cd6e840a56395831c353b4ed3173e404e5758e238a6896` | `outcome.txt` |
| Writer registration | exit 1, stdout empty, stderr `oo-pipeline: registration refused` | `writer.exit`, `writer.stderr` |
| Writer is actually writable | writer insert raises the audit to 1; reader insert returns SQLSTATE `42501` | `writer_insert=1`, `reader_insert=42501` |
| Connections | `pg_stat_activity` for `application_name = oo-pg-source` returns to 0 after the reader and after the writer | `connections_after_reader=0`, `connections_after_writer=0` |
| Draft composition | `captured-pg-draft` twice and `review-pg-draft` write the same 20832 bytes, exit 0, empty stderr, no trailing newline | `draft.stdout`, `draft-again.stdout`, `review.stdout` |
| Builder seals | `row_import` `not_proven`, `publication` `unavailable`, `grants` `not_authorized`, five columns with `pii` null, one `declared_fk` with null confidence and null review, one pending `candidate_relationship` | `outcome.txt` |
| Target | database list unchanged, `_oo` absent, `pg_publication` count 0, review opened no target | `databases_unchanged=1`, `target_opened=0` |
| Narrow exits | no args, unknown command, and an extra argument exit 2 with `oo-pipeline: usage`; invalid, strict unknown `target`, and a missing password file exit 1; the marker and the missing path are absent | `linux/probe-3/out/narrow/outcome.txt` |
| Existing import tests | 3 passed, 1 ignored, on the smoke container and again on the green probe | `import-existing.stdout` |
| Secrets | passwords, `PRIVATE KEY`, and the comment bodies are absent from the captured process files | `leak.exit` 0 |

`linux/probe-2/out/outcome.txt` recorded the same schema hash, the same comment hashes, and the same seals before Clippy failed that container. Its draft digest differs because `captured_at` is inside the catalog. The three builder outputs within each run are equal.

## Dependency and safety

No manifest or lock edit. The overlay is one integration test, `forbid(unsafe_code)`, and it was applied only to a copy of the archive. The product tree was not edited. `full_import.rs` was not edited. Clippy stderr still reports two unfulfilled `#[expect(dead_code)]` annotations in `crates/oo-connectors/vendor/sqlx-postgres`. The package command exited 0. Those files were not part of this commit.
