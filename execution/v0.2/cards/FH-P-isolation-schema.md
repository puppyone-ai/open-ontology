## Card FH-P — per-table isolation, partial publication, schema classification (S6-M-06, S6-M-07; D22)

Branch `v0.2/fh-p` from `1f267b6f5b95180139196fbb90c5b2e716d14699` (v0.2/field-hardening). Commit as the owner (no agent trailers), trailer
`Plan: S6-M-06 S6-M-07`. Read first: plan `specs/product-loops-v0.2-plan.md` §4 rows S6-M-06/S6-M-07, §5 T9 checklist,
D22; the field lessons summarised there (items 1–7). Your files: `tools/oo-load/**`, `crates/oo-pipeline/**` (migrations
numbered **0127–0134 only**), `tools/oo-test/t3t4.py` expectations, new `tools/oo-test/t9_isolation.py`. Do NOT edit
`deploy/lite/**`, `crates/oo-cli/**`, `crates/oo-gateway/**`, `tools/pg-builder/**`, `execution/**`, `specs/**`.

Defects (today's tree): `tools/oo-load/src/c5.rs` loops tables and returns Err on the first table error (e.g.
`c5_rebaseline_required_lineage` at the whole-table lineage check), so one table stops every table's publication; the
lineage is whole-table equality so any added column needs a rebaseline.

Build:
1. **Per-table isolation (S6-M-06).** In `oo-load sync` (C1 and C5 paths) a table-scoped error (named codes: schema
   drift classes, source permission, statement timeout, per-table budget, rebaseline required) marks that table
   `stale` with a named reason, retries it once in the same call, and continues. The stale table keeps its previous
   retained generation. Source-wide errors (connect, auth, probe, target) still fail the whole call as today.
   Contract (Q and R lanes consume it, keep it exact):
   - each per-table stdout line gains `"status":"fresh"|"noop"|"stale"`, and for stale `"stale_reason":"<code>"`
     and `"stale_detail"` (bounded, no credentials, no row data);
   - exit 0 = no stale tables; exit 3 = partial (≥1 table fresh or noop and ≥1 stale); exit 4 = every table stale
     (treat as failed cycle); other nonzero = global failure as today.
   - `oo-load publish` accepts a partial generation: stale tables are carried from their previous generation, the
     published release records `stale_tables: [{table, reason, since}]`, and `_oo.table_freshness` (or a new column in
     your migration) holds per-table state `stale` + reason + since so the Gateway can expose it (lane R reads it;
     document the column names in your report).
2. **Schema classification (S6-M-07).** Persist the sealed column catalog per table in the target as formal state
   (migration; not only a hash): published column name, ordinal, source type, nullability, charset/collation, key
   membership. Classify each change against the *published projection*:
   - `additive` (only trailing new columns) and `unpublished_only` (only unpublished columns change) → pass; new
     columns are NOT published (lane Q/G gates them behind review);
   - widening of a published column whose target type does not change (strings are all `text`, see engine_load_pg)
     → pass with no rebuild;
   - compatible widening that changes the target type (e.g. int→bigint, decimal precision up) → rebuild that table into
     a new physical generation and switch atomically at publish; the old generation serves until then;
   - drop/rename/type narrowing/nullability/key/ordinal change of a published column → table `stale` with
     `schema_drift:<column>:<kind>`;
   - no sealed catalog to prove against → `schema_unproven` (stale), never guess.
   Same rule at every call site (C5 layered, range parts, empty, C1 paths): one shared classifier, not copies.
   Oracle and MySQL fingerprints get the equivalent classifier (use the engine shape APIs, not PG-only SQL).
3. Tests: unit tests for the classifier (every class, per engine type mapping) and the isolation exit codes; update
   `t3t4.py` T4 expectations to the classes above (add_column → additive pass; widen string → pass; widen int →
   per-table rebuild; drop/rename → stale only that table, others publish); new `t9_isolation.py` live cases T9-01,
   T9-02, T9-03…T9-07, T9-11 on PostgreSQL 17, MySQL 8.4 and Oracle 23 (fixtures from `testing/engines`; labels
   oo.lane=fhp; at most two DB containers at once). Run `cargo test --locked -p oo-pipeline --lib`, oo-load tests,
   workspace clippy -D warnings (also `-p oo-connectors --all-features`), fmt, T3/T4 on PG17/MySQL 8.4/Oracle 23.
List files bound in `supply-chain/secret-review.json` that you changed. Report: commits, contract details (exact
field/column names), tests with counts, live results per engine, gaps.
