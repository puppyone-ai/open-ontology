## Card G-S5b — lane G: rerun the gold dialogue with the shared fixes

TODO: S5-G-01. Rebase on v0.2/product-loops first. The coordinator applied your numeric patch (decimal precision/scale
kept; uint64 numeric(20,0)) and lifted the retained Builder's call limit to a caller-set 1..4096 (instead of your
32-call proposal). Remove `tools/oo-test/T6-NUMERIC-FOLLOWUP.patch` and `T6-BUILDER-FOLLOWUP.patch`; apply the t6.py /
retained_cycle_live.py / README hunks of the Builder patch yourself, setting the Builder limit from the Builder's own
preflight batch count plus repairs (print it), not a constant.
Run `tools/oo-test/oo-test t6 --engine ENGINE --run-id RUN --execute` with the reviewed publisher on PostgreSQL 17.11,
MySQL 8.4.7 and Oracle 23.5: all 20 gold questions answered by the real-model Agent through the share prompt + native
CLI; record correct / wrong / NULL / truncated / refused per question honestly. Model files as on card G-S5. If a
question fails because of product behaviour (Gateway, Pack, Builder semantics), diagnose and name the owning component
with evidence; fix only in your own files.
