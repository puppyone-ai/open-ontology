# Findings

Reviewer: this session, after impl-5 and restart-2. No separate review agent. No product edit during the review. Scope stayed the frozen mirror.

## What the proof shows

The passing library selects `_oo.apply_receipts` and `_oo.schema_migrations` through a login that has no insert grant. Control install runs as `oo_cdc_offset_admin`. Runtime insert, select, and the watermark column update are the grants checked by `catalog_ok`. Install on the fresh schema returned `installed`, and the second call returned `replay`.

`Client::transaction` takes `&mut Client` in tokio-postgres 0.7.18. Read queries stay on a shared reference. The lost-commit lookup maps the driver error into the unit error of `lookup` with `map_err`. It does not open a second write when `CDC_OFFSET_RECONCILE=down`.

Numeric and `timestamptz` values are Rust strings. The extended query protocol sends them as text. The control statements cast `($n::text)::numeric` and `($n::text)::timestamptz`. The fixture does the same for the owner uuid and the receipt position. The impl-5 chain inserted receipts and moved the watermark, so those casts were accepted by PostgreSQL 17.10.

`same_batch` returns a bool. An equal hash is a replay and still re-evaluates the watermark. A different hash refuses and leaves the stored row. `live_chain` observed `OO010` from `commit_apply_receipt` on a changed payload and then a replay of the original hash.

A unique violation `23505` on the mirror insert is returned as `receipt_hash_conflict` without reading the stored hash. `CONTRACT.md` states the single-writer limit. A concurrent insert of the same hash would be labeled a conflict. The proof is single-writer.

Exit 0 with `checkpoint_advanced` false is a durable mirror. The partial, gap, and epoch cases in `live_chain` assert that. Exit 2 is a refusal that does not record the receipt. Exit 3 is `commit_unknown`.

The caller flags `source_gap` and `epoch_barrier` are still fields on the binding. The live helpers send false. The hole and the epoch change are detected from `expected_previous` against the stored watermark. The contract test covers that detection. The product still returns `source_gap` or `epoch_barrier` when the caller sets the flag, and that flag path was not given its own live call.

`external_xack` and `external_xtrim` are false on the asserted outputs. The sources under `src/` do not contain those commands. Checkpoint provenance is the caller-declared constant. The reader cannot select `target_generations`, so fence proof stays false.

## What remains parent work

Source acknowledgement, a real normalizer, Valkey `XACK` and `XTRIM`, the reject ledger, and large-segment integration are not this crate. Migration `0004` is not installed. The fixture executes `0000`, `0001`, and `0003` from the fixed tree and does not depend on the `oo-pipeline` crate. The moving `cdc-apply-pg` tree was not imported.

The ledger and the plan checkboxes for CDC-D05 were not changed. They are outside the owned paths.

## Attempts kept

impl-1 through impl-4 and the first restart directory still hold their logs and exit files. The first restart test passed; the script missed the captured line and exited 1. restart-2 is the green persistence run.

An uncommitted root `Cargo.toml` hunk adds workspace dependencies for axum, tower, and tower-http. This task did not write that hunk and does not commit it. a313 remains the root writer.
