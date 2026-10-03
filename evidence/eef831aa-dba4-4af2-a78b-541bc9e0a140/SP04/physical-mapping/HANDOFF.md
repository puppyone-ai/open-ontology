# Handoff

Fixed review is next. Source commit `b5415544c9eb8d608eafd411884627dda9ee7c84` on `02b9847159a68ced1443615cbb47bcb5872724b0`.

Owned paths:

- `crates/oo-pipeline/src/lib.rs` exports `physical_mapping`
- `crates/oo-pipeline/src/physical_mapping.rs` plans and quotes
- `crates/oo-pipeline/tests/physical_mapping_live.rs` is the wire-protocol test
- `crates/oo-pipeline/migrations/0004_physical_mapping.sql`
- `contracts/pipeline/physical-mapping-v1.md`

`0000` through `0003` are unchanged. The contract for the fence receipts was not edited.

The Rust API plans a binding and quotes a generated identifier. It does not open a connection. Persistence is `_oo.register_physical_mapping` and `_oo.lookup_physical_mapping`. Register is idempotent for an identical identity and returns a masked `OO022` when the digest or the stored row disagrees. The caller owns the transaction.

This task stores identifier bindings. It does not create user tables, copy user rows, write shadows, or insert receipts. Reader R4 and target apply are still pending. The evidence does not claim a full import.

Review focus: `0004` grants, the writer GUC `_oo.physical_mapping_writer`, parent checks, and the 240-bit physical prefix. The green Linux log is `linux/fixed/`. Failure consoles are `linux/fixed/attempt-1` through `attempt-5`. `FINDINGS.md` records the locale column, the restart method, and the catalog visibility of `prosrc`.
