## Card G-S2S3 — lane G: Self-build budgeter, community batching, entity registry, connectivity gate

TODO: S2-G-01, S2-G-02, S3-G-01, S3-G-02, S3-G-03. Lane label: `g`. Spec: REQ-BLD-01..04, AT-BLD-01/02.
Read: docs/product/03-selfbuild-context-budget.md (normative), docs/06-ontology-layer.md §4.11, crates/oo-builder
(limits.rs MAX_TABLES/MAX_COLUMNS, knowledge_budget.rs, incremental.rs, role_pack.rs), tools/pg-builder
(src/generate.rs, src/config.rs, scripts/library_selfbuild.py draft_batches and S5 prompt), tools/model-egress if present.

Deliver:
1. S2-G-01 budgeter: capability probe input (W, O_max, system-prompt overhead — the test endpoint injects
   ~2.5k tokens per call), I_avail = W − O_reserve − P_sys − margin (margin = max(2k, 10%·W)), O_reserve from
   per-item estimates (~70/column, ~250/table, ~60/relation) calibrated by EWMA on usage.completion_tokens,
   two-sided constraints, degrade order (fewer samples → compressed columns → column shards → recursive
   community split), NDJSON per-item output with stable IDs, missing-item re-request with bounded validation
   feedback, `unresolved` items to review, concurrency 1→2 by stream completeness, quota fail-fast keeping
   batch receipts keyed (model_id, prompt_template_hash, input_hash). Unit tests incl. a 300-column table
   that is not truncated.
2. S2-G-02: remove project-level MAX_TABLES=64 in favor of batch-level budgets; Python and Rust paths use
   the same budgeter (one implementation; the other calls it). T5 exit on a ~660-table fixture (lane H
   generator `--tables`) with full coverage.
3. S3-G-01: relation graph (declared FKs + S3 containment candidates + naming), shard-family/audit/backup
   folding, Leiden or Louvain communities bounded by the budget, hub signatures attached to referencing batches.
4. S3-G-02: three passes (in-community, cut edges, merge) with a global entity registry (stable id, canonical
   name, aliases, main/related tables) passed compactly to later batches; connectivity metrics (FK mapping
   rate with reasons, orphan-entity ratio, connected components, 3-hop reachability, naming conflicts) into
   the review summary; publish gate G14 per docs/06. `relationships/graph.json` in the Pack.
5. S3-G-03: per-table and per-community semantic dependency hashes; data-only cycles make zero model calls;
   schema changes re-run only affected communities, cut edges and merge items.
Live model: via tools/model-egress (or directly the OpenAI-compatible endpoint
`https://9r-admin.REDACTED/workflow/v1`, model `gpt-6.1-sol`, key file
`~/.config/open-ontology/secrets/model-key-9r-sol` referenced as a secret_ref — never print/log/commit it).
Keep live usage bounded: one full ~660-table run and one ~1000-table run at most, plus small runs.
Write scope: crates/oo-builder/src/** (new budget*/community* modules, incremental.rs, limits.rs, role_pack.rs
for graph.json), tools/pg-builder/** , tests. Prefix `builder`, `pg-builder`.
