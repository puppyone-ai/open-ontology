# Findings

## What the capture proves

A read-only `PgSession::catalog` document for `shop.customer` and `shop.purchase` compiles through `compile_captured_pg_catalog`. The draft keeps the composite primary keys and the foreign key `purchase_customer_fk` as a tentative `many_to_one` relationship. Email stays `text` with unknown PII and unknown semantic type. Reordering tables, columns, and privileges does not change the sealed bytes. Adding `note` makes the earlier column decision stale and leaves the curated label in the curated echo.

The legacy draft still says `pg_catalog_integration=not_accepted` and still names commit `b2d3cbc5af2b97d6565cbd67f0a9208f778a8c74`. The wrapper says `live_catalog_review=pending`. An override in the inner draft does not flip either statement.

## What it does not prove

No row was read. The reader has no `INSERT`. The wrapper says `row_import=not_proven` and `grants=not_authorized`. Catalog capture is not publication and not certification. There is no model call.

The comment body `a156-live-comment-body-do-not-carry` is present only as the fixture sentence in `pg/init.sh`. The captured catalog stores byte length 35 and sha256 `e2b8d8558b447b6d8a87baa45536610bfc282c42f1c7418b11ccc6068da183a7`. The body is absent from the catalog JSON, both drafts, and both wrappers. `a156_owner` is present on the catalog document and absent from the drafts and wrappers.

## Failures kept

| Record | Exit | Cause |
|---|---|---|
| `linux/smoke/attempt-1` | 101 | Archive omitted `examples/ontology-draft`, so the golden test could not open `failures/adapter.code`. |
| `linux/focused/attempt-1` | 1 | Rustfmt 1.94 reflowed historical `draft.rs` lines. Later runs check only the three new or newly helper-touched files that were formatted for 1.94. |
| `linux/focused/attempt-2` | 101 | `selection_from_catalog` exposed private `SelectionIn`. Replaced by `catalog_bindings`. |
| `linux/focused/attempt-3` | 101 | CLI fixtures under `contracts/builder/structure-draft/fixtures` were outside the archive. |
| `linux/focused/attempt-4` | 1 | A fresh container had not installed clippy for 1.94.0. |
| `probe/attempt-1` | 101 | Edition 2024 rejected `Err(&str)` in `Result<(), String>`. 39 errors. Dependencies had already compiled. |
| `pg/audit-attempt-1.txt` | 1 | Classifier counted only `statement:` lines and rejected `START`. |

The green runs are the current `linux/smoke/`, `linux/focused/`, and `probe/out/` files, plus `pg/audit.txt`.

## Audit detail

Reader traffic is extended-protocol `execute sqlx_s_N` plus two simple `START TRANSACTION ISOLATION LEVEL REPEATABLE READ, READ ONLY` statements and two `ROLLBACK` statements. `set_config` appears inside `SELECT`. Disconnect lines `could not receive data from client` are not statements. Reader write count is 0.

## Left for the next review

The wrapper's `live_catalog_review` is `pending`. This evidence does not accept the historical `not_accepted` field as a new approval, and it does not mark SP07 complete. The probe workspace lived under `/tmp/oo-a156-eef831aa/probe-snapshot` and was not added to the product `Cargo.toml`.
