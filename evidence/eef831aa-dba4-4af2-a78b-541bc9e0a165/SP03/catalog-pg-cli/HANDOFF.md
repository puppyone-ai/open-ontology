# Handoff

Author delivery of `oo-pipeline catalog-pg` on `34498e75d5876d74ddb5343b1b0f5b9b1adaf1df`, tree `a99506b163e7b0dfcbdcbc4d8236c69546593a94`, parent `a173c58a9f90a10b7853ddc1c3fcb74d1b0befa1`.

The next session can review that commit. Coordinator commit `be2a8fe59540e675eaee1e95b0fdc14bc6aeb6e5` landed after it and before this evidence record. That coordinator commit does not accept `catalog-pg`. The command reads `oo.pipeline.catalog-pg/v1` from stdin and writes the `SourceCatalog` from `PgSession::catalog` after `registration_allowed`. Stdout is `oo.source.pgsql/v1` with no trailing newline. The contract is `contracts/pipeline/catalog-pg-cli-v1.md`. The requirement record is `REPORT.md`. The fresh process oracle is `linux/post-3/out/`. Smoke before the edit is `linux/smoke-before/`. Failed exits are `linux/post/` and `linux/post-2/`.

`oo-builder captured-pg-draft` and `oo-builder review-pg-draft` accepted that stdout in-process. The two captured runs and the review run wrote the same bytes. PII stayed unknown, the foreign key stayed a tentative candidate, `row_import` stayed `not_proven`, and `publication` stayed `unavailable`.

Reuse a162 (`3fba40dd9fdebaa219ab4eb7fdb39b8fcf1a1e4a`) for the import command this change left in place. This run did not repeat the import live matrix. The non-ignored import CLI tests passed on the green container.

This file is not an approval of publication, a grant, a type certificate, CDC, or a parent checkbox. Nothing was pushed. `CLEANUP.txt` records removal of this task's container, volumes, network, and `/tmp` tree. Foreign containers were left in place. `docker prune` was not run.
