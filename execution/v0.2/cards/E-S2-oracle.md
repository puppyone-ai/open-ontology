## Card E-S2 — lane E (Oracle): LogMiner C1 (online_catalog), C5

TODO: S2-E-01. Lane label: `e`. Spec: REQ-ENG-03, AT-ENG-01. Decisions D8 (online_catalog only, R1).
Requires: your S0/S1 work integrated (rebase on v0.2/product-loops first).

Deliver: `ChangeSource` for Oracle with a platform-owned LogMiner reader: self-managed log file list for
19c+ (no CONTINUOUS_MINE), CONTINUOUS_MINE allowed on 11g, CDB/PDB with c## common user, SCN watermarks,
online_catalog dictionary only; structure changes the online catalog cannot decode → per-table
rebaseline. earliest_available() from the oldest archived/online log still present. Chunk SCN window
correction; C5 digests via digest_range with the four layers; no-change cycle = zero payload/rewrite/
generation. Reuse tools/oracle-preview and tools/job-worker native_oracle* code; don't duplicate.
Prove zero source writes for C1 with Oracle unified/standard auditing in the test container (T7 slice): only
SELECT and LogMiner package calls appear, no DML/DDL by the reader account.
Also close the ORA-01555 gap from your S1 run: when undo is exhausted at the chunk's SCN, re-read that
chunk at a fresh SCN and rely on the chunk-window log correction (or C5 digest re-check) for consistency;
prove zero diff on the undo-stress test instead of stopping at 7,846/12,000.
Tests: 23ai C1 + C5 three cycles incl. no-change; 11g/21c C5 three cycles (C1 on 11g/21c if feasible,
else record why). 19c remains blocked.
Write scope: crates/oo-connectors/src/oracle/**, crates/oo-connectors/tests/oracle_*, tools/oracle-preview/**,
tools/job-worker/src/native_oracle*.rs, testing/engines/oracle/** (not seed/). Prefix `connectors/oracle`.
