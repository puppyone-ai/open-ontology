## Card M-S4b — lane M: publish oo-load generations under the Gateway's contract

TODO: S4-M-01. Rebase on v0.2/product-loops first (your 33300eb16…043ebc3e3 are integrated as …f5ca7b5a8).
Your report found the blocker: `_oo_load` views/receipts do not satisfy the Gateway's `_oo` publication and policy
contracts (`_oo.native_library_publication`/`native_library_head`, `native_library_source_ready`, `physical_mappings`,
`table_freshness`, `source_watermark`, policy catalog/public surface). This card grants what you need to close it.
Granted write scope (in addition to your earlier scope): `crates/oo-gateway/src/{policy*.rs,public_surface.rs,
publication_binding.rs,query_executor.rs,pack_reload.rs}` and new files there; one NEW migration in
`crates/oo-pipeline/migrations/` (next number; never edit existing migrations); `deploy/lite` scheduler/worker wiring.
Not granted: `crates/oo-gateway/src/auth_*.rs` (lane B), `crates/oo-cli/src/admin/**` TUI/build (lane A is building
`oo build` on top of your publication entry — expose it as one library function plus `oo-load publish` CLI and name
them in your report).
Prefer the adapter direction: each verified oo-load generation is published by writing the existing `_oo` contract rows
(an immutable library publication + head with per-table/C5 receipts, physical mappings to the generation's views,
freshness = source data-as-of, source watermark) so the Gateway's reviewed F22/policy code stays unchanged; change the
Gateway only where the contract cannot be met, with tests. Write the mapping table (oo-load object → `_oo` contract row
and field) into your report as proposed docs text for docs/product/02 §6 / docs/07-gateway.md.
Exit (S4-M-01, card M-S4 + E191): Gateway query client running continuously across three consecutive PostgreSQL 17.11
cycles — zero errors, each query consistent with the generation it started on (publication id / data vector headers),
`/v1/freshness` advancing each cycle, old generations released per policy only when unheld; one MySQL 8.4.7 and one
Oracle 23.5 cycle; then in deploy/lite the scheduler runs `oo-load sync` → data-only Builder → publish, and after one
scheduled cycle `oo status` reports all seven services/jobs healthy. Record evidence.
