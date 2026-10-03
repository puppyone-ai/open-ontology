# Handoff

APPROVE LIMITED the `physical-mapping/v1` primitive at `b5415544c9eb8d608eafd411884627dda9ee7c84`. Green run `0c476cb4-eb51-4d3a-b8f9-0d417ecd6509` is `linux/fixed/` and `linux/probe/`. Failure logs stay in `linux/fixed/attempt-1` and `linux/fixed/attempt-2`.

Reviewed files, unchanged by this review:

- `contracts/pipeline/physical-mapping-v1.md`
- `crates/oo-pipeline/migrations/0004_physical_mapping.sql`
- `crates/oo-pipeline/src/lib.rs`
- `crates/oo-pipeline/src/physical_mapping.rs`
- `crates/oo-pipeline/tests/physical_mapping_live.rs`

`0000` through `0003` match the parent `6766126b8e7c0916d6740ba75601698d2ebd0fb2`. The Rust plan does not open a connection. Persistence is `_oo.register_physical_mapping` and `_oo.lookup_physical_mapping`. Register is idempotent for an identical identity. The caller owns the transaction.

The table owner can insert a check-valid row while `_oo.physical_mapping_writer` is `register`, including a digest that `register_physical_mapping` would reject with `OO022`. The trigger in `0004` does not recompute SHA-256. Runtime and agent cannot insert. The probe rolled the owner row back. A later change can close that owner path in the trigger. This limited approval does not require it for the runtime primitive.

Still open: Loader, fenced `COPY`, reader R4, user-table DDL, shadow rows, receipts written by this migration, and a full import. SP04-W05 and AT-PL-05 are not signed by this slice. The a140 evidence manifest was not verified. No second architecture and no license review.

Slot B was not used. Cleanup of run `0c476cb4-eb51-4d3a-b8f9-0d417ecd6509` removed this task's containers, network, and cargo, target, and Postgres volumes.
