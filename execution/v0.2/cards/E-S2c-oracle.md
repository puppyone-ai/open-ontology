## Card E-S2c — lane E: Oracle T7 under D11a, strict C5 T7, probe-first fix

TODO: S2-E-01. Rebase on v0.2/product-loops (decision D11a, commit a34c383bb and baseline revision 10).
D11a category: DML on LogMiner-owned objects whose name starts with LOGMNR in schema SYS or SYSTEM, plus
COMMITs in the same session while a DBMS_LOGMNR call or LogMiner query is in progress. The reader account's
top-level statements must be only SELECT and DBMS_LOGMNR calls. Anything else fails — including the
"trace-visible auditing/maintenance writes" you saw: if they are not LOGMNR* objects in SYS/SYSTEM, they fail
and must be explained in the evidence (e.g. audit trail self-writes by the audit facility itself belong to the
audit infrastructure, not the reader — prove attribution from the audit columns, or leave them failing).
Work rule: smallest local workaround and keep going; commit as you go.
1. Implement D11a in your T7 classifier; re-run T7 for C1 on 23 / 21c / 11g.
2. Strict zero-write T7 for the default C5 path with its own reader account (no LogMiner): must pass with zero
   writes of any kind on all three versions.
3. Fix probe-first planning on 23.26.3 (`ora_snapshot_unavailable`): onboarding order is probe → plan → read.
Exit: per version, C5 T7 zero-write PASS; C1 T7 PASS under D11a or a precise list of remaining
non-attributable records.
