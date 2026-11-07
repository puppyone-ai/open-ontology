## Card E-S5d — lane E: Oracle T3/T4

TODO: S5-E-04 (exit: all pass or honest downgrade; AT-ENG-02). Rebase on v0.2/product-loops first: lane N's T3/T4
suite is integrated (`tools/oo-test` t3t4, `tools/oo-test/T3T4-VALIDATION.md`), together with its C5 durable-range and
table-shape lineage fixes (Oracle C5 inherits durable ranges but was not run) and your typed SourceEngine.
1. Add Oracle to the t3t4 suite (extend, don't fork): source restart mid-chunk, network drop mid-C5 (and mid-C1 on 23
   with the D11a opt-in), target restart mid-publish, SIGKILL mid-cycle, redo/archivelog retention expiry → explicit
   rebaseline, resetlogs/incarnation change → epoch refusal; T4 add/drop/rename column, widen NUMBER/VARCHAR2, add/drop
   table, PK drop/add — Oracle C5 must include a table-shape lineage like N's MySQL/PG fix.
2. Run on 23.5, 21.3 and 11.2 (11 via the thick worker). Each case resumes or refuses by name with zero divergence after
   rebaseline. Disk gate as in N's harness; build prerequisites first (dispatch deletes Cargo targets after each run).
