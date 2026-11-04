## Card M-S4c — lane M: continue M-S4b to its exit (publication entry, Gateway, lite)

TODO: S4-M-01. Rebase on v0.2/product-loops first: your retained generations (7f33b3d96, 638d3588e, migration 0122) are
integrated. Card M-S4b's grants and exit apply unchanged — read it again (`execution/v0.2/cards/M-S4b-gateway-publication.md`).
This is a long card: the previous run stopped after the prerequisite with time left. Do not stop at a milestone; work
through all steps below, committing after each, and stop only when the exit holds or a step is proven impossible.
1. `oo_pipeline` publication function + `oo-load publish --generation <id>`: writes your mapping table's `_oo` rows for
   one retained generation in one transaction (library publication + head with per-table/C5 receipts and a data vector,
   physical_mappings to the generation views, source_watermark, table_freshness from measured source time or NULL),
   idempotent, refusing unverified/sealed-mismatch generations. Unit + owned-PG tests.
2. Gateway: start a real oo-gateway over the published Agent DB (lane A's S2f boundary harness shows how), run an
   authenticated query client continuously across three PostgreSQL 17.11 source cycles (source changes → `oo-load sync`
   → publish): zero errors, publication-id/data-vector headers consistent per query, `/v1/freshness` advancing. Change
   Gateway code only where the contract cannot be met, with tests.
3. Retention: release generations older than the policy only when no publication/query holds them.
4. One MySQL 8.4.7 and one Oracle 23.5 cycle through the same publish path.
5. deploy/lite: scheduler runs `oo-load sync` → data-only Builder → publish; `oo status` seven healthy after one cycle.
