# L7 formal evidence (spec 80 section 8, as amended by D81)

No new formal item for L7 under D81. FM3 multi-cloud events was planned for L7 and is dropped by D81; event semantics are covered by `events_live`, `prefix_sync`, the adapter unit tests and the L7 acceptance script.
The fast checks of the existing models (typecheck, simulate, FAST verify, conformance, kani-fast) stay green in CI;
latest full run: `evidence/L4/formal.md` (integration 724e7915e). L7 formal gate under D81: green.
