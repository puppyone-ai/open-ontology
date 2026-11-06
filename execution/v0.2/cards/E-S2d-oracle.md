## Card E-S2d — lane E: D12 attribution, Oracle LOB support, Oracle T1 via oo-load

TODO: S2-E-01, S1-E-01. Rebase on v0.2/product-loops first (D12 at 6f9fa1149/f676468bd; oo-load and the T1 suite
are integrated under tools/oo-load and tools/oo-test). Work rule: smallest local workaround and keep going; commit as you go.
1. D12 in your T7 classifier (plan §9 D12, docs/product/05 T7): fail on any reader top-level DML/DDL/DCL or
   non-allowlisted call, any change to a non-system-schema object, any unattributable record. Attribute and list per
   category: session-state statements (ALTER SESSION SET), Oracle bookkeeping on system schemas (SYS.USER$ logon
   update, SEQ$/SEG$ maintenance, AUDSYS.AUD$UNIFIED / SYS.AUD$ audit trail), D11a LogMiner objects and their
   DBMS_LOGMNR_INTERNAL recursive blocks. A blank-object INSERT must be attributed from audit/trace columns
   (owner, object, recursive depth, SQL id) or it fails. Re-run C5 and C1 T7 on 23.26.3 / 21.3 / 11.2.
2. LOB support: the T1 fixture's TYPE_EDGES LOB columns fail with unsupported_column_type in the Oracle adapter. Add
   bounded CLOB/NCLOB/BLOB reads (size limits from ReadBudget, chunked fetch, truncation never silent — over-limit
   values fail the chunk with a clear error), Arrow mapping, sha256 coverage, and the thick worker path for 11g.
3. Oracle T1 against the Agent DB: `python3 tools/oo-test/t1.py --engine oracle --versions 23,21,11 --run-id ...`
   (11g through the thick worker), full scale, concurrent writer, corrective re-read, no-rewrite resume; T8 if the
   suite has source metrics by then.
Write scope: crates/oo-connectors/src/oracle/**, tests, testing/engines/oracle/**, tools/oracle-preview/**, the Oracle
hooks in tools/oo-test (coordinate: lane M edits the generic T1/T8 hook — keep your changes to Oracle-specific files).
