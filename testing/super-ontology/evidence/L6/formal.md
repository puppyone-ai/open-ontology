# L6 formal evidence (spec 80 section 8, as amended by D81)

No new formal item for L6 under D81. FM6 drift refresh, FM9 and KP9 were planned for L6 and are dropped by D81; drift, refresh and media chunk resume are covered by `refresh_changes_live`, `reprocess`, `conformance_jobs`, the media unit tests and the L6 acceptance scripts.
The fast checks of the existing models (typecheck, simulate, FAST verify, conformance, kani-fast) stay green in CI;
latest full run: `evidence/L4/formal.md` (integration d9b66e6e8). L6 formal gate under D81: green.
