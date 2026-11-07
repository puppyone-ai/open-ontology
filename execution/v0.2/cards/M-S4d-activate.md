## Card M-S4d — lane M: activate oo-load generations in the Gateway (D15), then three live cycles

TODO: S4-M-01. Rebase on v0.2/product-loops first (your 87e87d755…19d59eb3f are integrated). Decision D15 (plan §9)
settles the blocker you reported: add shard consistency `retained_generation`.
Scope this run to steps 1–3 and do not stop before step 3 has run live (commit after each step):
1. NEW migration: `_oo.native_library_check_shard` gains a `retained_generation` branch that verifies the shard against
   your candidate ledger (recipe sha, sealed state, view definitions/columns, data vector) — no legacy full_import/ACK
   reads; `native_library_source_ready` treats it as an immutable cut (watermark/freshness rows you write must match).
   Owned-PG tests: valid activation; each mismatch refused with OO030.
2. Gateway: `publication_binding::library_cut` treats `retained_generation` like `per_table` (immutable, independent of
   ingestion), with a unit test; no other Gateway change unless a test proves it necessary. `oo-load publish` then calls
   `native_library_mark_ready` after the fences are open; in tests open fences the way the Builder publisher does (see
   tools/pg-builder publisher and `oo_publish_target.fence`), never by bypassing them.
3. Live: real oo-gateway (lane A's `deploy/lite/test_operator_boundary.py --gateway-boundary` harness shows startup
   with mTLS) over a PostgreSQL 17.11 Agent DB; three source cycles (change → `oo-load sync` → `oo-load publish`) while an
   authenticated client queries continuously: zero errors, `x-oo-publication-id`/data-vector consistent per query,
   `/v1/freshness` advancing. Record evidence.
Steps 4–5 of card M-S4c (MySQL/Oracle cycles, retention, lite seven healthy) follow in the next card.
