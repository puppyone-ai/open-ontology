## Card G-S6-catalog-meta — carry declared source metadata into the lite retained build

TODO: S3-G-02 (T5 "every foreign key mapped or justified") on the L1 path; found by S6-B-01 rehearsal pass 4 (ledger).
Branch `v0.2/g-catalog-meta` at `25a6d23b88faf05bf405c3eda150454046cc93ac`. Commit on this branch (owner identity already configured; NO Co-Authored-By or
agent trailers; message trailer `Plan: S3-G-02 S6-B-01`). Do not touch execution/, specs/, docs/ baselines.

Defect: on the lite `oo build` path (D16: stages run in the runtime image), `tools/pg-builder/scripts/retained_build.py`
observes only the retained Agent DB copy. `inventory()` hard-codes `comment=None`, `foreign_keys=[]`, and nullability comes
from the copy (so source primary keys show nullable). Rehearsal evidence: `../rel/evidence/rel-rehearsal-4/REPORT.md`
(section "Gaps and follow-ups"): a 3-table fixture with 2 declared FKs and comments produced 1 inferred relationship,
no comments, nullable PKs, an isolated entity.

Goal: declared source metadata (table/column comments, primary keys, NOT NULL, declared foreign keys) reaches facts,
the Builder draft prompts, and the owner review input, for every engine whose SourceEngine catalog reports it.
- Rust: in `crates/oo-cli/src/admin/build_retained.rs` (and wherever the lite build has the source connection; see
  build_lite.rs, doctor.rs for how oo-cli reaches `oo_connectors` — D19 allows that only through oo-connectors), read the
  source catalog with the engine-generic catalog API (`crates/oo-connectors/src/catalog.rs`, `model.rs` ForeignKey etc.),
  scoped to the stack's included tables, under the existing source budgets; write `source-catalog.json` (0600) beside
  builder-config.json and pin it in the config as an optional key `source_catalog: {path, sha256}`, the same pin shape
  `pinned()` already verifies for origin files. Read-only; no new source privileges (check doctor GRANT lists).
- Python: `retained_build.py` accepts the optional pin (add to OPTIONAL), verifies it with `pinned()`, maps source
  tables to retained datasets (same names the loader uses; refuse with a named error on mismatch), and: sets comments,
  primary keys and nullability in `inventory()`; feeds declared FKs into `lib.build_facts` (it already has
  relationship/candidate machinery: declared FKs become relationships with a declared provenance and still get
  containment observed; an FK whose containment fails is kept with its observation, not dropped); makes comments
  available to the draft prompts the same way library_selfbuild does for its catalog input if it has such a path.
  Without the pin, behaviour is unchanged (data-only reuse digests must not change for existing deployments: check
  `semantics.data_only_digest` inputs and keep them stable, or justify in the report).
- Tests: Python unit tests next to the existing `test_library_*` / retained tests (fixture catalog with 2 FKs and
  comments → facts relationships include both, inventory has comments and non-null PKs, mismatch refusal, no-pin path
  unchanged); Rust unit test for the catalog export shape. Run: `cargo test --locked -p oo-cli`, `cargo clippy
  --workspace --locked --all-targets -- -D warnings`, `cargo fmt --all -- --check`, `cd tools/pg-builder/scripts &&
  OO_LIBRARY_BUILDER_BIN=<built oo-builder> python3 -m unittest` for the library_* and retained modules, and
  `python3 ci/check_generated_docs.py` (regenerate docs/guide and docs/reference with tools/docs/*.py if line numbers
  moved). If `supply-chain/secret-review.json` binds a file you changed, the secret gate will fail: list such files in
  the report (do not edit that JSON).
- Live check: one PostgreSQL 17 fixture (label oo.lane=g, 3 related tables, 2 FKs, comments) through the retained build
  script directly (no model call needed if the script can stop at facts/inventory; otherwise use the harness in
  tools/oo-test that T5 uses) showing both FKs and comments in facts.json and review-input.json. Remove containers.
Final report: commits, design, test results with counts, live-check output summary, any digest-stability note.
