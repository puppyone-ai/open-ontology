## Card M-S2b — lane M: MySQL and Oracle incremental sync and T2; PostgreSQL T2 on aligned pins

TODO: S2-C-01, S2-C-02, S2-E-01, S2-D-01 (T2 exits). Rebase on v0.2/product-loops first: your S2 commits are integrated
(ce8567ad1), and the source_epoch accessors you asked for are in (291e62129). Lane H is aligning testing/infra/images.pins to
one patch version per major (MySQL 5.7.44/8.0.44/8.4.7/9.5.0, PostgreSQL 13.23/14.24/15.19/16.15/17.11/18.6) — use those
versions; if H's change is not on the integration branch yet when you run, use those tags explicitly and say so.
Deliver (work rule: keep going; commit as you go):
1. MySQL in oo-load sync: C1 under `--features mysql-binlog` (persist source_epoch with the position; refuse a different
   epoch), C5 four-layer cycles, C5→C1 handoff barrier; T2 on 5.7.44 (amd64), 8.0.44, 8.4.7, 9.5.0 (binlog enabled in the
   fixture), with T8.
2. Oracle in oo-load sync: C5 by default; C1 only with explicit opt-in (D11a), source_epoch = DBID/resetlogs; T2 on 23 /
   21 / 11 (11 through the thick worker), with T8.
3. Re-run PostgreSQL T2 on the aligned versions so every P0 major has T1 and T2 on the same build.
Engine fixes belong to lanes C/E: if you need one, implement the smallest change in the engine file (granted for this
card), with a unit test, and name it in the report.
