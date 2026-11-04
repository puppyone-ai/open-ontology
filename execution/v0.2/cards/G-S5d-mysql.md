## Card G-S5d — lane G: MySQL gold dialogue rerun

TODO: S5-G-01. Your E309 runs gave PostgreSQL 20/20, Oracle 20/20 and MySQL 18/20 with two upstream model timeouts.
Rerun only MySQL 8.4.7 once, unchanged (`tools/oo-test/oo-test t6 --engine mysql --run-id RUN --execute`), at most two
DB containers of yours at a time and full cleanup before you start. Report all 20 per-question results; do not merge
results across runs — this run stands on its own. Then remove the stale `T6-*.patch` files if any remain.
