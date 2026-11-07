## Card M-S2 — lane M: incremental apply into the Agent DB and the real T2 suite

TODO: enables the T2 exits of S2-C-01, S2-C-02, S2-D-01, S2-E-01 (AT-ENG-01) and the S2 sprint exit. Lane label `m`.
Context: after the S1 exit the coordinator integrates the engine lanes' S2 code — MySQL ChangeSource (feature
`mysql-binlog`), C5 range cycles and C5→C1 handoff; PostgreSQL pgoutput ChangeSource and four-layer C5; Oracle LogMiner
ChangeSource (explicit opt-in, D11a) and four-layer C5. They were proven against in-memory targets only.
Deliver (work rule: smallest local workaround and keep going; commit as you go):
1. `oo-load sync` (or a sibling subcommand): from a recorded L0 / last durable position, consume `ChangeSource::next_batch`
   and apply row changes to the Agent DB in source commit order with tombstones, durable position + ACK only after the
   target commit; resumable after SIGKILL without duplicate apply. C5 mode: run the engine's four-layer C5 cycle and apply
   range replacements through the existing staging → atomic replace path; a no-change cycle writes nothing and creates no
   new generation (measure target bytes and generation counters).
2. Handoff: C5 → C1 barrier per the playbook (subscribe L0, bounded repair, log-first with tombstones, switch at L_end);
   lineage break / purged logs → explicit rebaseline, never silent.
3. T2 in tools/oo-test: for each engine/version, a concurrent writer between cycles; three C1 cycles (where the engine
   supports C1) and three C5 cycles incl. one no-change cycle; zero diff vs source after each cycle (comparison oracle),
   monotonic positions, zero payload/rewrite/generation on no-change cycles; T8 sampled during cycles.
   Run: PostgreSQL 13–18 (C1 + C5), MySQL 5.7.44/8.0.44/8.4.7/9.5.0 (C1 with `mysql-binlog` + C5), Oracle 23/21/11
   (C5 by default; C1 with explicit opt-in on 23/21/11).
Write scope: tools/oo-load/**, tools/oo-test/** (T2 hook), crates/oo-pipeline/src/**, new migrations after 0119 if needed.
Engine code belongs to lanes C/D/E: request fixes there through your report; minimal glue only.
Disk: run one engine container at a time.
