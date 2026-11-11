## V3-I-06 — run the model-backed acceptance to completion
Branch `v0.3/integrate`. Read tools/oo-test/V3-I-05-VALIDATION.md; do not redo its passed steps. Commit as the owner, no agent trailers, trailer `Plan: V3-I-06`.

Containers: there is NO count limit for this card (the earlier 4-container cap was the coordinator's mistake; lite bootstrap needs 5). Label everything oo.lane=v03, remove all of it at the end, never touch other containers.

1. Demo bank fix: q02 is denied for the analyst role by design. Either run it as the role that may see it or move it to the permission track; the load-time preflight from V3-I-05 must pass. Do not change the gold answers.
2. Model-backed own-database onboarding (model files `~/.config/open-ontology/secrets/model-endpoint-9r` and `~/.config/open-ontology/secrets/model-key-9r-sol`, by path only through the product's file sources; never print/commit/log contents): fresh `npm pack` install, empty HOME, MySQL 8.4 with the ERP/OO-Logistics fixture you used, `oo init` non-TTY end to end: recommended tables → model Self-build → adjudication → `oo ontology graph --format text` → hash-bound confirm → publish → profile → `oo check` → 3 `oo query` + 1 `check_answerable`. Then the same in a zh 120x40 pty to the deliver screen; save the transcript. Per-stage durations and time to first answer. If any stage fails, fix the product and rerun.
3. Live A/B `--arms c0,c3` with the 40-question ERP bank against the pack published in step 2, plus the fixed demo bank and the permission/injection tracks; seed 42, pinned model id; replay must reproduce byte-identically. Report relative change, fixed/broken lists, health counters. Do not tune questions after seeing results.
4. Regression for touched code + QS-01..05 once more; scan evidence for secrets/private terms; keep no docker inventories naming unrelated resources.
Report: commits, stage timings, A/B tables, regression, residuals.
