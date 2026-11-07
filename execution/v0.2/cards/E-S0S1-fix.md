## Card E-S0S1-fix — lane E: commit, 11g through a thick-mode worker, real ORA-01555

TODO: S0-E-01, S1-E-01. Lane label: `e`. This follows your previous run (STATUS partial, nothing committed).

1. Commit your work now in logical commits (rules, engine adapter, keyset/ROWID chunking, real-engine test
   runner, fixtures). The 5 file_worker_parent failures are pre-existing on the base (common rule 9) and do
   not block commits — name them in the body.
2. Your shared-file request (Oracle plan checks through V$SQL_PLAN, not write-producing EXPLAIN PLAN) is
   accepted; the coordinator applies it to the docs. Remove testing/engines/oracle/shared-file-request.patch
   from your tree (do not commit it). Make sure the code never issues EXPLAIN PLAN.
3. Oracle 11g: the pinned thin driver rejects 11.2 servers. Implement the documented external-driver worker
   path (REQ-SRC-11 P2): an amd64 Linux worker image built under testing/engines/oracle/worker/ (or the
   product location the docs prescribe for driver workers) with Oracle Instant Client 19.x (thick mode; 19c
   clients support 11.2 servers) — obtain Instant Client only from Oracle's public download/yum endpoints
   that need no login; if a download requires sign-in, stop and report blocked. The worker speaks the same
   Arrow IPC chunk protocol back to the adapter. Run T0/T1/T8 on 11.2.0.2 through it. Record Instant Client
   license terms in the evidence (do not redistribute it in git).
4. Real ORA-01555: in a test container shrink the undo tablespace / undo_retention, run a long chunk under a
   concurrent writer, and show the shrink-and-retry path continues from flushed rows only with zero diff.
5. T1 against Agent DB is pending the coordinator's loader adapter (S1-M-01); keep the component comparison
   and state that clearly.
Exit: commits on the branch; 23ai/21c/11g T0/T1/T8 with evidence; ORA-01555 test evidence.
