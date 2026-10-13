# DL01-W1-26 fixed review

Reviewer session `ca2d8367-93ee-42b0-9956-69232e2d84d8`. Author content `280bc35da56b1d7d018c54e79b136a1f99c7e1ab`. Seal `d6a7e414a40768c03618c0bd74d74503ab218ef8`. Base `577160c22de63adb9b38e0147f3bc2da893c4d9d`.

Decision: `approve`. Approved scope: the frozen `source-capture-policy` planner child at that content commit.

The prior review commit `d9060704a7a924b9d2ffedffe23ba1dd859fdc9c` remains `request_changes` on content `647b8b2e2b9f3966b9496bcef41aaba75c23e682`. Its raw decision `raw/probes/unbound-table.stdout` and `raw/build/smoke.exit` 10 are unchanged.

This run sealed a spool at LSN `5/3D4E5`, epoch 3, and read commit position `55340232242603742437` from `cdc_spool::recover`. A ledger-only `cdc-offset-store` receipt for `ledger_g4` at that position checkpointed `insert_first`. The same plan’s `finance.payment` / `payment_g2` method position `77` has `repair_allowed` false, `advance_capture_position` false, `transactional_allowed` false, and `consistency_claim_allowed` false, with reasons `repair_denied` and `receipt_unbound`. Plan `repair_allowed`, `advance_capture_position`, `transactional_allowed`, `same_instant_claim_allowed`, and `consistency_claim_allowed` are false. The operator is `refuse`. The join warning is `CROSS_TABLE_SKEW`. Raw decision: `repair/raw/probes/ledger-only.stdout`.

A pair whose receipts both use that recovered position, with both generations required, checkpointed `insert_first` and continued. Missing required receipt, sibling position `77`, sibling epoch 4, source `treasury`, foreign schema digest, and a selected generation absent from the receipt list fail closed. Each present-receipt case matches an in-process `cdc_offset_store::checkpoint` result. The absent selected generation is `refused` / `selected_receipt_absent` and is not given the present receipt’s `insert_first`.

`plan.rs` `table_offset_reason` (lines 205–276) returns `receipt_unbound` when the table generation is neither selected nor required (line 226). `assemble` (lines 313–360) sets source repair and transactional flags only when every table’s flags are true (line 334) and sets the operator to `refuse` when a table reason is in `closes_receipt` (line 472 includes `receipt_unbound`).

Compile, `cargo test`, and Clippy exits are 0 inside one Linux arm64 Rust 1.94 builder. Probe exit is 0. Author source was not edited.

Packet: `evidence/ca2d8367-93ee-42b0-9956-69232e2d84d8/DL01/capture-fixed-review`.

Outside this approval: parent DBX-W03, loader, scheduler, live offset mirror, GTID set comparison, `receipt_closed` re-export, the source matrix, and a 14-night product run. Prefix `oo-dl01w1-26`. No database. Container `oo-dl01w1-26-review` was removed. The pinned image, network `oo-dl01w1-26-net`, and volumes `oo-dl01w1-26-cargo` and `oo-dl01w1-26-target` remain.
