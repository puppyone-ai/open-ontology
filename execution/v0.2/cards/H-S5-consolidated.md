## Card H-S5 — lane H: one consolidated P0 evidence run with explicit verdict rows (S5 exit)

TODO: S5-H-01, S5-H-02 (S5 exit: "P0 全版本 T0–T8 证据包；矩阵由证据生成"). Rebase on v0.2/product-loops first.
Lane M's collector (`tools/evidence-index/`, on branch v0.2/m-pipeline — `git show v0.2/m-pipeline:tools/evidence-index/REPORT.md`)
found that most suites that passed never wrote explicit per-version verdict rows, so the generated matrix shows P0 as
experimental. Fix that at the source and rerun:
1. Every suite runner (T0 runner, T1 t1.py, T2 t2.py/t2_p0.py, T3/T4 t3t4, T7 audit, T8 meters) writes rows in the
   `execution/v0.2/evidence-index.json` row schema (specs/scripts/gen_support_matrix.py: target, version, suite,
   variant c1/c5/snapshot, result pass/fail/skip, run_id, sha256 of the raw result, emulated, observed_at UTC) into its
   evidence dir as `index-rows.json`; `oo-test matrix` merges them. No inferred passes.
2. Run `oo-test matrix` for every P0 version — MySQL 5.7.44 (amd64), 8.0.44, 8.4.7, 9.5.0; PostgreSQL 13.23–18.6;
   Oracle 23.5, 21.3, 11.2 (19c blocked, E014) — suites T0, T1, T2 (c1 and c5), T3, T4, T7, T8. Sequential, disk gate
   ≥ 20 GB before each target, build prerequisites first (dispatch deletes Cargo targets after a run). Expect hours;
   keep going.
3. Commit the merged rows to `execution/v0.2/evidence-index.json` (granted), run `specs/scripts/gen_support_matrix.py`
   (+ `--check`, `--self-test`) and commit the generated matrix; report the version × capability levels and every
   failure or downgrade.
T5/T6 (model-backed) are a separate step; do not run them here.
Rerun note (E327): your first attempt found every target disk-blocked (< 20 GB). The coordinator pruned unused build
cache (free 28 GB). Rebase (your 77c39378e, 022f02a25 stay), then run the matrix again. Between targets remove each
target's containers and volumes before starting the next, and do not pull images that are not already present unless
free disk is ≥ 25 GB. If disk blocks a target again, wait up to 30 minutes for other lanes to free space before marking
it blocked.
