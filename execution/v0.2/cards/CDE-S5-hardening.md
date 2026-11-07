## Card <LANE>-S5 — lanes C, D, E: T3 fault injection and T4 schema change on every P0 version

TODO: S5-C-04 (MySQL 5.7/8.0/8.4/9.x), S5-D-03 (PG 13–18), S5-E-04 (Oracle 23ai/21c/11g; 19c if unblocked).
Spec: AT-ENG-02. Use oo-test suites T3/T4 (add the engine-specific hooks).
T3: kill the worker mid-chunk and after target commit; restart the source container; toxiproxy cut and
throttle; SSH jump disconnect (with lane A's transport); logs purged past retention; PG 18 slot
invalidation. Exits: resume without duplicate apply; retention exceeded → explicit rebaseline; every
failure leaves a receipt. T4: add column, change type, create table, drop table, rename — data-only cycles
make zero model calls, structure changes trigger only targeted Self-build (with lane G). Record any version
that cannot pass, downgrade honestly.
