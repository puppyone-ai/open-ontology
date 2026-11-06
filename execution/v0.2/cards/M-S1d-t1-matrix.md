## Card M-S1d — lane M: MySQL T1, T8 source metrics, split_chunk for MySQL/PostgreSQL

TODO: S1-M-01, and the T1/T8 exits of S1-C-01 / S1-C-02 / S1-D-01. Rebase on v0.2/product-loops (your oo-load and
T1 suite are integrated at 3cb3e823c; SourceEngine::split_chunk added at b9527ce40).
Work rule: smallest local workaround and keep going; commit as you go.
1. MySQL T1 against the Agent DB with `oo-load`: 5.7.44 (amd64 emulated), 8.0.44, 8.4.7, 9.5.0 on the lane-H fixture at
   full scale (2,000,000-row table, concurrent writer, corrective re-read, no-rewrite resume). The MySQL adapter is
   integrated (crates/oo-connectors/src/mysql); default features only (no mysql-binlog needed for T1).
2. T8 in the T1 suite: measure source-side peak reader connections and aggregate read duty cycle during the load
   (MySQL processlist / PG pg_stat_activity / Oracle v$session for the reader user, sampled), plus target write bytes;
   fail when above the ReadBudget. Re-run PostgreSQL 13–18 T1/T8 on the current tree (your previous full matrix
   predates a22ed9b56) and MySQL.
3. Implement SourceEngine::split_chunk for MySQL (wrap its existing inherent split_chunk) and PostgreSQL keyset
   chunks; use it from oo-load instead of whole-table replans; test with the growing-table fixture.
Oracle LOB support (`unsupported_column_type` on TYPE_EDGES) is lane E's next card — do not edit
crates/oo-connectors/src/oracle/**; keep Oracle T1 marked pending.
Write scope: tools/oo-load/**, tools/oo-test/** (T1/T8 hooks), crates/oo-pipeline/src/**, and the split_chunk
impls in crates/oo-connectors/src/{mysql,postgres}/ (minimal, report them).
