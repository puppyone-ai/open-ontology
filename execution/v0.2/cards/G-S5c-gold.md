## Card G-S5c — lane G: gold dialogue rerun with composite keys

TODO: S5-G-01. Rebase on v0.2/product-loops first: lane K's composite entity keys are integrated (OOS, Pack compiler,
owner review, reviewed-community compiler, publisher fence binding; evidence `k-composite-replay-20261104` replays your
three g-s5b review inputs successfully). Rerun card G-S5b unchanged: `tools/oo-test/oo-test t6 --engine ENGINE
--run-id RUN --execute` with the reviewed publisher on PostgreSQL 17.11, MySQL 8.4.7 and Oracle 23.5 — all 20 gold
questions through the share prompt + native CLI, per-question correct/wrong/NULL/truncated/refused, product defects
diagnosed to their owning component with evidence. Model files as on card G-S5.
