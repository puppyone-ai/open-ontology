## Card C-S2c — lane C: MySQL ChangeSource (C1) and the remaining S2 exits

TODO: S2-C-01, S2-C-02. Rebase on v0.2/product-loops first: your futures-util request is applied
(7bfa559f4, optional edge under the non-default `mysql-binlog` feature, already-locked 0.3.34).
Your S2b commits stay on your branch (not integrated until the S1 exit).

Deliver, committing as you go (work rule: smallest local workaround and keep going; `blocked` only if none):
1. `ChangeSource` for MySQL under `--features mysql-binlog`: binlog ROW events (TABLE_MAP / WRITE / UPDATE /
   DELETE, transaction boundaries, GTID events incl. tagged GTIDs), earliest_available() from the actual
   oldest binlog (your retention helper), next_batch() bounded by ReadBudget, positions as Watermark.
2. Chunk-window log correction at connector level: for keys seen in a chunk's [low, high) window the log wins,
   tombstones prevent resurrection; L0 recorded before the first full read and the log consumed during it.
3. Handoff barrier C5→C1 over multiple sources/bindings (L0 → bounded repair → log-first with tombstones →
   switch at L_end), GTID lineage break or purged logs → explicit new-generation rebaseline error.
4. Empty-chunk contract (52f965813): an empty chunk must push one schema-bearing empty batch included in the
   IPC hash — add a test and fix the MySQL adapter if needed.
5. Real-engine tests on 5.7.44 / 8.0.44 / 8.4.7 / 9.5.0 with binlog enabled in the fixture container: T2 C1
   three cycles incl. no-change; window correction under a concurrent writer; handoff; purge → rebaseline.
Agent DB acceptance comes from lane M's `oo-load`/T1 suite; keep connector-level evidence and mark the rest pending.
