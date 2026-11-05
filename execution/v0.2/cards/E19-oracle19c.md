## Card E19 — lane E19: Oracle 19c, the last P0 version (unblocks E014)

TODO: S0-H-04 (19c pin), S1-E-01/S2-E-01/S5-E-04 for 19c. The owner created the Oracle account, accepted the registry
terms and logged Docker in to container-registry.oracle.com (decisions/E365). Commit trailers: `Plan:` only — no
Co-Authored-By (owner instruction). Lanes T (audit/T7 files), H (t1.py) and G (t6) are editing tools/oo-test
concurrently — add 19c through resolver/pins and hooks, not by editing their files.
1. Pin `container-registry.oracle.com/database/enterprise:19.19.0.0` (or 19.3.0.0 — check which manifest has arm64;
   amd64 runs under emulation) by digest in testing/infra/images.pins (lane H's format) and make `oo-test` resolve
   Oracle 19 (CDB/PDB, common user setup as for 21c/23ai). Record the image license acceptance as owner-done (E365).
2. Run on 19c with explicit verdict rows: T0 (product doctor via t0.py), T1/T8 (full scale), T2 C5 three cycles and
   C1 opt-in LogMiner under D11a three cycles, T3/T4 via `oo-test t3t4` (archivelog expiry: inject with RMAN — the
   enterprise image has it), T7 via `tools/oo-test/t7.py` (snapshot, c5, c1). Merge rows (`oo-test matrix --merge-only`).
Report the 19c version × suite table.
