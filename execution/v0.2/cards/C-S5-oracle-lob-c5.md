## Card C-S5L — lane C: Oracle C5 digests over LOB columns

TODO: S5-E-04, S5-H-01. Rebase first. Commit trailers: `Plan:` only — never Co-Authored-By. Your keyless fix is
integrated. Oracle 23.5 corrected T1 fails before NO_KEY because keyed `TYPE_EDGES` raises
`lob_source_digest_requires_streaming` (reproduced on base b96ce601c): Oracle C5 cannot digest ranges containing
CLOB/BLOB/NCLOB columns.
Fix: compute Oracle range digests over LOB columns by streaming (DBMS_CRYPTO.HASH is not available to a read-only
reader without EXECUTE grant — prefer reading LOB content in bounded pieces client-side with DBMS_LOB.SUBSTR / chunked
fetch inside the range read and hashing on the client, within ReadBudget; or document the grant if unavoidable and
make doctor print it). Unit tests; then corrected T1 (tools/oo-test/t1.py) at full scale on Oracle 23.5, 21.3, 11.2
(11 via the thick worker) must pass, plus an idle C5 cycle with zero writes.
