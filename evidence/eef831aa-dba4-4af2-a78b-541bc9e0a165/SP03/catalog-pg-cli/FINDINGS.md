# Findings

Author record for `34498e75d5876d74ddb5343b1b0f5b9b1adaf1df`. The green gate is `linux/post-3/`. Independent review has not run.

## Public session API was enough

`PgConnector::connect`, `PgSession::inspect_access`, and `PgSession::catalog` are public. `registration_allowed` is a public field. `SourceCatalog` serializes as `oo.source.pgsql/v1`. The command uses that path. It does not call `prepare_snapshot`. `PgSession::shutdown` stays `pub(crate)`. Dropping the session is the public release, and `linux/post-3/out/outcome.txt` records that `pg_stat_activity` for `application_name = oo-pg-source` returned to 0 after the reader catalog and again after the writer refusal.

No connector, core, builder, migration, lock, or manifest edit was required.

## Compile failures kept

`linux/post/` inside exit 101. The catalog test called `PgConnection::connect_with` without `sqlx::Connection` in scope. Import tests, captured-pg-draft CLI tests, and review-pg-draft CLI tests passed in that container. The live test was not started.

`linux/post-2/` inside exit 101. The non-ignored catalog tests passed. Clippy rejected `customer.foreign_keys.len() != 0` (`clippy::len_zero`). The live test was not started.

`linux/post-3/` is the green rerun. The same private PostgreSQL container was reused. Its data directory was not replaced.

## Captured draft and review draft are the same bytes

`linux/post-3/out/draft.stdout`, `draft-again.stdout`, and `review.stdout` are 18764 bytes and compare equal. Neither file ends in a newline. The wrapper says `row_import` `not_proven`, `publication` `unavailable`, `grants` `not_authorized`, `source_connection` `not_used`, and `live_catalog_review` `pending`. Provenance says `publication` `unavailable` and `pg_catalog_integration` `not_accepted`. Four generated columns have `pii` null. Four `pii_unknown` queue items are `pending`, `publishable` false, detail `pii_unreviewed`. The one relationship is `declared_fk`, `join_hint` `left`, `confidence` null, `review` null. One `candidate_relationship` item is `pending`, `publishable` false, detail `join_hint_left_unverified`.

`oo-pipeline` does not depend on `oo-builder`. The test spawns the sibling binary built in the same container.

## Catalog bytes

Reader stdout is 4757 bytes, protocol `oo.source.pgsql/v1`, source `a165src`, database `oo_a165_src`, `captured_at` length 27 ending in `Z`, schema hash `30ddee6dfef5c60f3d30ae251048b99d673b9ccfce37631685c08ce153fb215f`. An admin query of `pg_attribute`, `pg_index`, and `pg_constraint` matched columns, primary-key order, and the foreign key `order_customer_fk`. Comment objects are `byte_len` and `sha256` only. Those hashes match SHA-256 of the fixture texts. `oracle.json` stores the hashes. A scan of the process files found none of the fixture texts and none of the password files. The CA fingerprint is the only certificate material kept here.

The writer principal exited 1 with empty stdout and stderr `oo-pipeline: registration refused`. The same test then inserted as that principal and recorded SQLSTATE `42501` for the reader insert.

## Vendor noise

Clippy stderr repeats two unfulfilled `#[expect(dead_code)]` annotations in `crates/oo-connectors/vendor/sqlx-postgres`. `cargo clippy -p oo-pipeline --all-targets -- -D warnings` still exited 0. Those files were not edited.

## Clippy component

The pinned Rust image does not contain clippy. Each builder container ran `rustup component add clippy --toolchain 1.94.0` into its own writable layer. The layer is discarded when the container is removed. The installs did not overlap, and no host `rustc` or `cargo` was used.
