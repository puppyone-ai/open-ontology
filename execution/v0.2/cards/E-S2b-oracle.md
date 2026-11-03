## Card E-S2b — lane E: Oracle S2 remaining exits under decision D11

TODO: S2-E-01. Rebase on v0.2/product-loops first (your S2 commits stay on your branch until the S1 exit).
Decision D11 (plan §9, docs/database-support/04 §5.3): Oracle C1 is "R1 with Oracle-internal recursive temporary
writes". Work rule: smallest local workaround and keep going; commit as you go.
1. T7 audit for Oracle: admit exactly one explicit category — statements at recursive depth > 0 whose objects are
   only SYS.LOGMNR_GT_*, plus session-setting statements issued by the driver (ALTER SESSION time zone/NLS);
   everything else (any DML/DDL by the reader, any other object) fails. Keep the 41 records you saw as a fixture
   for a test that proves the classification. Do not suppress records; classify them in the evidence.
2. Disclosure: probe() reports the LogCdc capability as NeedsSetup with a Requirement of kind Feature that
   explains the LogMiner temporary writes and requires explicit opt-in; nothing may select Oracle C1 implicitly
   (`sync: auto` stays C5 for Oracle). Doctor (lane A) renders Requirements generically — no CLI change needed.
3. Oracle 21c XE: make audit collection actually return records (unified vs standard auditing in XE 21; check
   the container's audit settings) so T7 can be evaluated; record what was needed.
4. Oracle 11g: extend the thick worker protocol to allow the LogMiner package calls and V$ views the C1 reader
   needs (allowlist, still no DML/DDL), then run C1 three cycles on 11.2.
5. Four-layer C5 scheduling for Oracle (time hint, recent window, chunk digest, rare full read); no-change cycle
   = zero payload / rewrite / generation (component level until lane M's oo-load lands).
Exit: 23ai, 21c, 11g C1 + C5 three cycles with zero diff; T7 per version with only the D11 category admitted.
