## Resume V3-I-10 after a host OOM/reboot

The host ran out of memory and Docker restarted while your previous run was in progress; it is not a product defect. Your commits are on this branch (f949a0b32..c8e17cdf2) and tools/oo-test/V3-I-10-VALIDATION.md is untracked work in progress. Review and keep what is correct; do not redo committed or recorded steps. Memory budget: keep at most ONE lite stack and ONE source database running at a time, and stop containers between A/B seeds. A v0.2 regression stream (one Oracle/PG container at a time, lanes m/h/t) runs concurrently; never touch its containers. Commit in small steps.

## V3-I-10 — make the reviewed ontology help, measurably and stably
Branch `v0.3/integrate`. Read tools/oo-test/V3-I-08-VALIDATION.md and V3-I-09-VALIDATION.md. Commit as the owner, no agent trailers, trailer `Plan: V3-I-10`. Whole repo in scope. Containers: no limit; label oo.lane=v03; clean up; never touch others. Model files by path only (`~/.config/open-ontology/secrets/model-endpoint-9r`, `~/.config/open-ontology/secrets/model-key-9r-sol`); never print/commit/log contents.

Facts: ERP A/B was C0 19 / C3 18 (I-08b first), then 19/24 after 109004a40, then after 1f8419feb 20/16 (seed 42) and 20/15 (seed 7). C3 is unstable and currently worse. That is the core value claim of the product, so treat it as a P0 product defect.
1. Diagnose with evidence, not guesses: for every question that differs between arms or runs, diff the C3 prompt (the pack content the model saw), the SQL produced, and the gateway result. Bucket causes (wrong pack facts, misleading wording, missing join path, metric definition, size/noise of the pack prompt, refusal triggered by pack text, Gateway admission). Compare compiled packs at 109004a40 vs 1f8419feb (rebuild both from the same source) and quantify which change moved which questions.
2. Fix product causes in the compiler/pack/builder (and the eval prompt assembly only if it misrepresents what an Agent actually receives via the CLI/MCP). Never edit questions/gold; never add question-specific hints.
3. Measure with variance: 3 seeds × both arms on the 40 ERP questions (+ demo), report mean, min/max and per-question stability; replay byte-identical. Target: C3 ≥ C0 on every seed and mean relative change ≥ +15%; if not reachable, report the ceiling and the remaining causes.
4. lg02/lg08 must be answered or explained by a documented product limitation with a fix plan.
5. Regression for touched code + QS-01..05.
Report: commits, cause buckets with counts, pack diffs, A/B table per seed, regression, residuals.
