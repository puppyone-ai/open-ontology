## Card E-S3 — lane E: SQL Server engine (snapshot + C5, CT/CDC)

TODO: S3-E-02. Rebase on v0.2/product-loops first. Write scope: `crates/oo-connectors/src/sqlserver/**` (new),
`testing/engines/sqlserver/**` (new), a `sqlserver` feature in `crates/oo-connectors/Cargo.toml` (granted; Cargo.lock may
change only for a driver crate — prefer `tiberius` with rustls; run `cargo deny --config ci/deny.toml check advisories
licenses` and `cargo audit` and report results), and an oo-load arm with unit test.
Deliver (keep going; commit as you go):
1. SourceEngine for SQL Server 2019 / 2022 / 2025 (amd64 images under emulation; pin one patch per major through lane
   H's images.pins format): probe/capabilities (edition, CT/CDC availability, snapshot isolation), source-ordered keyset
   chunks with source collation, read budgets, watermark (CT version / CDC LSN), digest_range.
2. ChangeSource: Change Tracking when enabled, CDC when enabled; neither is enabled by the product (doctor prints the DBA
   statements). Persist source_epoch (database GUID + recovery fork).
3. T0, T1 (oo-load Agent DB comparison, concurrent writer), T2 (C5 and one of CT/CDC), T7 (read-only: Extended Events or
   audit attribution per D12) and T8 on all three versions. At most two DB containers at once; SQL Server needs ~2 GB RAM
   each — run versions sequentially.
4. A short `testing/engines/sqlserver/SUMMARY.md`; proposed doc text for docs/product/05 in your report (coordinator applies).
