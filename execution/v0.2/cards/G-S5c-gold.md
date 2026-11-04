## Card G-S5c — lane G: gold dialogue rerun with composite keys

TODO: S5-G-01. Rebase on v0.2/product-loops first: lane K's composite entity keys are integrated (OOS, Pack compiler,
owner review, reviewed-community compiler, publisher fence binding; evidence `k-composite-replay-20261104` replays your
three g-s5b review inputs successfully). Rerun card G-S5b unchanged: `tools/oo-test/oo-test t6 --engine ENGINE
--run-id RUN --execute` with the reviewed publisher on PostgreSQL 17.11, MySQL 8.4.7 and Oracle 23.5 — all 20 gold
questions through the share prompt + native CLI, per-question correct/wrong/NULL/truncated/refused, product defects
diagnosed to their owning component with evidence. Model files as on card G-S5.
Rerun note (E309): since your last run the integration branch has the Gateway `+HH` offset fix, clarified wording for
Q05/Q12/Q13/Q14 (gold unchanged), typed Oracle SourceEngine columns (rebaseline Oracle), negative-scale decimals and
`model_egress.py --timeout`; set the Agent's egress timeout to 300 s. Keep everything else unchanged.
