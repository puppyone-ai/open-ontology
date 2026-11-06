## Card G-S6-catalog-meta-2 — source catalog export (Rust) for the lite retained build

TODO: S3-G-02, S6-B-01. Same branch `v0.2/g-catalog-meta`, continue from `554ba3f1c2ebde30575123a85d4d1141a5d06b1f` (your pass 1). Commit rules as before
(owner identity, no agent trailers, `Plan: S3-G-02 S6-B-01`). Read your pass-1 report
`evidence/g-catalog-meta-20261106/REPORT.md`.

Coordinator decision: the shared SourceEngine interface change in
`evidence/g-catalog-meta-20261106/source-api-prerequisite.patch` is APPROVED as proposed (additive, default `Ok(None)`).
1. Apply it. Implement `declared_catalog` for PostgreSQL (comment TEXT via pg_description/obj_description/col_description
   — the existing catalog only hashes comments; add text collection here, bounded by the read budget's byte limit with a
   named refusal when exceeded), MySQL (information_schema TABLES/COLUMNS COMMENT, KEY_COLUMN_USAGE/REFERENTIAL_CONSTRAINTS)
   and Oracle (ALL_TAB_COMMENTS, ALL_COL_COMMENTS, ALL_CONSTRAINTS/ALL_CONS_COLUMNS). Use only views covered by the
   existing doctor GRANT lists per engine/version (`crates/oo-cli/src/admin/doctor.rs` and docs/reference DBA grants);
   if a version needs more, return Ok(None) for it and say so. Other engines keep the default.
2. CLI wiring in `crates/oo-cli/src/admin/build_retained.rs` (+ whatever opens the source in the lite build): for the
   stack's included tables call declared_catalog; on Some write `source-catalog.json` (0600) beside builder-config.json
   and pin `source_catalog: {path, sha256}`; on None omit the pin (unchanged behaviour). Comments are untrusted data:
   they flow only into facts/prompts as data, as your Python side already treats them.
3. Tests: Rust unit tests for the export shape and the budget refusal; existing suites (`cargo test --locked -p
   oo-connectors --all-features --lib`, `-p oo-cli`, `-p oo-pipeline --lib`), clippy workspace (-D warnings, also
   `-p oo-connectors --all-features`), fmt, python library/retained modules, `python3 ci/check_generated_docs.py`.
4. Live: PostgreSQL 17, MySQL 8.4 and Oracle 23 fixtures (labels oo.lane=g; at most two DB containers at once; Oracle
   via the repository's existing Oracle fixture method in testing/engines/oracle) with 2 FKs + comments each: call the
   export through the CLI build path or a focused integration test, and run retained_build facts/inventory on the PG one
   end to end (model replay is fine) showing FKs, comments, non-null PKs in facts.json and review-input.json.
List every changed file that `supply-chain/secret-review.json` binds (do not edit it). Final report: commits, tests with
counts, live results per engine, gaps.
