## Card H-S5c — lane H: T1 must judge the concurrent load (review M1)

TODO: S5-H-01 (re-opened by the independent review, execution/v0.2/reviews/S6-M-03-L1-L3-a07ebfa01.md M1). Rebase
first. Commit trailers: `Plan:` only — no Co-Authored-By. Lane T edits audit/T7 files; lane G runs t6 — avoid those.
Problem: `tools/oo-test/t1.py:712-728` never compares the concurrent load; it stops the writer, runs a fresh full
reload ("corrected") into the same relation and compares only that, so a chunker that drops or duplicates rows under
concurrent writes still passes.
Fix: after the concurrent snapshot, stop the writer and run ONE incremental correction cycle through the product
(`oo-load sync` C5 for all engines; that is the documented consistency-B correction), with no full reload; compare that
result to the source. Also add a negative test that injects a dropped and a duplicated row into a loaded chunk and must
fail the comparison. Keep the full-reload path only as an explicitly named diagnostic, never as the verdict.
Rerun T1/T8 on all 13 P0 versions at full scale (2M events, 200 tables) with explicit rows, merge, regenerate the matrix.
