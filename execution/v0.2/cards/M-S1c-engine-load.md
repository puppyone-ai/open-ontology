## Card M-S1c — lane M: oo-load and the real T1 suite (blockers cleared)

Both blockers from your M-S1b run are cleared on the integration branch (cd9cce88e): SourceError::budget and
::protocol are public, and SourceEngine::read_chunk now requires an empty chunk to push one schema-bearing
empty batch (Oracle fixed; PostgreSQL already did; MySQL — if its adapter does not, add a contract test and a
minimal fix in crates/oo-connectors/src/mysql/engine.rs, which this card grants you, and report it).
Your 898878776 is integrated. Rebase on v0.2/product-loops first.

Work rule for this card: if you hit another shared-interface gap, implement the smallest local workaround in
your own write scope, write the proposed shared patch into your report, and KEEP GOING to the exits. Report
`blocked` only if no workaround exists at all. Commit as you go.

TODO: S1-M-01 (remaining exit), enabling AT-ENG-01 T1 for S1-C-01 / S1-D-01 / S1-E-01. Lane label: `m`.
Context: your previous run (integrated as effdc19ba) added C5 write metering, the receipt→typed adapter and
the SIGKILL recovery harness, but "engine-neutral admission and an actual fake-SourceEngine-to-target test
remain incomplete". Integrated engines now: PostgreSQL (crates/oo-connectors/src/postgres), Oracle
(crates/oo-connectors/src/oracle incl. the 11g thick worker). MySQL lands soon on the same interface.

Deliver:
1. Engine-neutral admission: any `SourceEngine` (crates/oo-connectors/src/engine) chunk stream —
   `plan_chunks` → `read_chunk` → ChunkReceipt + Arrow — goes through the existing staging → atomic replace
   → publish path into an Agent DB (PostgreSQL target), with the receipt's sha256/rows/bytes validated and
   persisted, plan_revision/growth splits honored, watermarks recorded per chunk. No MySQL-only assumptions;
   respect each engine's semantics flags (Oracle empty string = NULL). Unit test with a fake SourceEngine.
2. A product entry point `oo-load` (binary in an existing product crate or tools/, your call; document it):
   `oo-load --stack <stack.yaml|json> --source <id> [--tables ...] --target <agent-db secret_ref config>`,
   resumable, reporting per-table receipts as JSON.
3. Real T1 suite in tools/oo-test: add the T1 hook that seeds the lane-H fixture, starts a concurrent
   writer, runs `oo-load` into a fresh Agent DB container, applies the chunk-window correction available
   for the engine (or reruns affected chunks), and compares source vs Agent DB with the oo-test comparison
   oracle (canonical normalization, per-chunk ordered hash, first differing key). Run it for real on
   PostgreSQL 13–18 and Oracle 23ai / 21c / 11g (11g through the thick worker). Record T1/T8 evidence per
   version (T8 includes target write bytes from your meter).
Write scope: crates/oo-pipeline/src/**, new migration files after 0119 only if needed, tools/job-worker/src/**,
tools/oo-test/** (T1 hook only — keep lane H's existing API), the new oo-load entry point. Prefix `pipeline`,
`oo-test`, `oo-load`.
