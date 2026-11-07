## Card G-S5e — lane G: T5/T6 on every remaining P0 version, with verdict rows

TODO: S5-G-01 (exit: P0 all versions). Rebase on v0.2/product-loops first. Done so far: PostgreSQL 17.11 20/20,
Oracle 23.5 20/20, MySQL 8.4.7 19/20. Disk is no longer a constraint (≈450 GB free).
1. Make t6 (and the reviewed Builder run that precedes it, as T5) emit explicit verdict rows through
   `tools/oo-test/verdicts.py` (suite T5: Builder T5 completeness — every table/column item resolved or reasoned;
   suite T6: pass only when all 20 answers are correct; record per-question detail in the evidence) and merge them with
   `oo-test matrix --merge-only`.
2. Run, sequentially, one T5+T6 per version: MySQL 5.7.44 (amd64), 8.0.44, 9.5.0; PostgreSQL 13.23, 14.24, 15.19,
   16.15, 18.6; Oracle 21.3, 11.2 (thick worker). Same model files and limits as card G-S5; egress `--timeout 300`.
   Build prerequisites first. A run that fails only on `UPSTREAM_TIMEOUT` may be repeated once, as a separate run that
   stands on its own (never merge answers across runs); record both.
3. Commit the merged evidence rows (granted: `execution/v0.2/evidence-index.json`) and regenerate the matrix
   (`specs/scripts/gen_support_matrix.py`, `--check`, `--self-test`).

Resume note (E368): the coordinator session restarted and your previous run was cut off mid-way. Your worktree keeps your commits and any uncommitted work: review it, commit what is correct, rebase on v0.2/product-loops, and continue the card to its exit. Build prerequisites first. Commit trailers: `Plan:` only, no Co-Authored-By.
