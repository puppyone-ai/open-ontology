## Card T10-mitigate — land the long-snapshot mitigation and finish T10 (S6-H-02; D24)

Branch `v0.2/t10` (this card is committed on the branch; never delete it). Read `tools/oo-test/T10-LIVE4.md` and
`execution/v0.2/cards/T10-production-impact.md`. Live-4 proved a real product defect: changed C5 on PostgreSQL held
one long snapshot over hot OLTP tables (reader xmin age 1.1M-2.1M, n_dead_tup 0.84M-1.59M, CPU 413%, foreground p95
30-83 s). The same class threatens MySQL (history list) and Oracle (undo).

1. Foreground rate (harness): after calibrate()+settle(), run every phase at 50% of the settled sustainable rate
   ("moderately loaded production"); record capacity, settled and used rates. Keep everything else in the method.
2. Product mitigation 3 first (it is the measured defect), for C5 reads AND initial load on PostgreSQL and MySQL:
   per-chunk short read transactions (bounded rows/bytes/seconds per transaction, keyset ranges), so no source
   snapshot outlives one chunk. Consistency: C5 already compares digests and corrects before publication; the
   chunked read must be proven publication-correct the same way (a row changed between chunks is either captured or
   corrected by the next C5 cycle, never published inconsistent with its generation's digest proof). Keep the single
   snapshot mode as an explicit option; make chunked the lite default only if the correctness tests prove it.
   Unit tests for chunk bounds and the correction path; live T1 and T2 at full scale on PG 17 and MySQL 8.4.
3. Mitigation 2 (adaptive throttle on source activity: PG active sessions, MySQL Threads_running, Oracle active
   sessions) on by default with bounded backoff; unit tests.
4. Fix the harness preparation defects seen in live-4: C1 preparation exceeding `mysql_repair_staging_budget`,
   the PG fixture slot invalidation (size `max_slot_wal_keep_size` on the T10 source only and record it), and the
   Oracle 900-second preparation/initial timeouts (size them from measured load rate; never mask a product error).
5. Re-measure all phases (baseline, initial, initial_duty100, c1, c5 idle/changed at both policies) for Oracle 23,
   MySQL 8.4, PostgreSQL 17; before/after numbers for each mitigation. Oracle: undo sizing guidance from measured
   load duration; prefer C5 over LogMiner C1 if LogMiner CPU exceeds budget; document standby/replica reads.
6. Update both "Production impact" docs with measured numbers and recommended settings.

Unrelated containers on the host are idle and must never be stopped, inspected beyond `docker stats`, or modified;
never write their names anywhere. Commit as the owner, no agent trailers, trailer `Plan: S6-H-02`.
Report: per engine × phase table, mitigations with before/after, correctness re-runs, commits, tests, residuals.
