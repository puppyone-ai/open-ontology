# Handoff

Independent review of `oo-pipeline catalog-pg` on `34498e75d5876d74ddb5343b1b0f5b9b1adaf1df`, tree `a99506b163e7b0dfcbdcbc4d8236c69546593a94`, parent `a173c58a9f90a10b7853ddc1c3fcb74d1b0befa1`.

Verdict: **APPROVE LIMITED**. The requirement record is `REPORT.md`. The line-level notes are `FINDINGS.md`. Author evidence `ce06598c` is not this proof. Import proof remains a164 on `3fba40dd9fdebaa219ab4eb7fdb39b8fcf1a1e4a`.

The deciding oracle is `linux/probe-3/out/outcome.txt`, `linux/probe-3/out/reader.stdout`, and `linux/probe-3/out/draft.stdout`. Pre-overlay smoke is `linux/smoke/`. Failed harness runs are `linux/probe/` and `linux/probe-2/`. `linux/probe-2/out/` is an earlier live pass whose container then failed Clippy. Do not read it as the deciding oracle.

The overlay under `overlay/` was applied only to a copy of the git archive. The product tree was not edited. `full_import.rs` was not edited. `Cargo.lock` was not edited.

Parent SP03, parent SP07, a full policy, and publication are not approved. The draft still says publication is unavailable, grants are not authorized, PII is unknown, the foreign key is tentative, and row import is not proven. Nothing was pushed. `CLEANUP.txt` records removal of this task's container, volumes, network, and `/tmp` tree. Foreign containers were left in place. `docker prune` was not run. `MANIFEST.sha256` is the SHA-256 of every file in this directory except itself.
