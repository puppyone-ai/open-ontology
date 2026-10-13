# ClickHouse original C4 native contract

Status: required integration contract, not an implemented gate or acceptance.
C5 whole-typed connection evidence is retained separately and cannot sign C4.

## Original required exits

- Spec §7A.4 REQ-SRC-10(1,2,4,6), lines 386–395: declare order and consistency; only Total positions may support ordering/deduplication/repair claims; C4 replaces a partition in one Agent DB transaction. S0/B does not imply cross-table transactions.
- `docs/database-support/03-change-capture-and-consistency.md` lines 34,104,132,142: enumerate actual partitions and modification identities, reread changed partitions, atomically replace each partition; absence within a refreshed partition is physical deletion. Deletion outside refreshed partitions can remain invisible and must be disclosed. Non-Total position codecs only serialize continuation, never expose a fake global comparison.
- Profile §9.6 (`04-database-profiles.md:275`): real `system.parts`, `FINAL`, asynchronous lightweight DELETE/mutations, readonly/quota/execution budget; preserve UInt64, Decimal256, Nullable, LowCardinality and default-for-missing semantics.
- Plan SP04-W05/W06 (`open-ontology-v0.1-plan.md:367,373`): target-exclusive fence/lease, atomic data/quality/receipt; durable completion afterwards; same-generation replay and payload conflict, stop source writes and full unsampled diff0. Original full fixture includes all twelve columns.
- AT-SRC-08/09 (`open-ontology-v0.1-spec.md:1209`): invalid/noncomparable positions, schema/epoch recovery, deletion and configured reconciliation periods; report actual freshness/capture/grade. AT-PL-05/06/09 (`:1225`): commit/ACK/crash, exact full cells, no overwrite of later delete, new sensitive column hidden and destructive schema change paused.

## Reuse and present gap

Reuse `scripts/c4_capture.py::cycle` and the real `LiveSource.inspect` in `run_c4_live.py`: source table UUID, registered schema fingerprint, mutations pending, active partition IDs plus signatures (part names/rows/bytes/modification time), disappeared partitions, and FINAL reads. `product-run-14-c4` proves its original five-column acceptance shadow, rollback, replay and 3s/7s elapsed periods. It is not a twelve-column native runtime proof and not a resident scheduler.

Native full12 raw typed Arrow is already captured and admitted. The existing native partial-update path requires an ordered nonzero scalar position. A ClickHouse partition signature is not such a position. We must not synthesize an offset from row count/time/job pin or mark a whole C5 snapshot C4.

## Required executable source-to-loader interface

79 owns the concrete accepted API/target migration. The following is the source input semantics, with field spelling subject to its single-writer implementation:

1. A table binding names actual source ID, native table UUID/epoch, logical table, exact twelve-column schema/type and source generation/parent. Capture method is `c4_partition`; deletion is `partition_replace`; order is `PerPartition`; grade is S0/B and cross-table false.
2. Each refresh names the actual partition ID, previous acknowledged signature (or null for first import), observed current signature (null for a disappeared partition), exact source-produced FINAL Arrow for that partition and the approved native key list. No unrefreshed partition's rows may be dropped or overwritten. Empty/disappeared partitions are first-class inputs and cannot require a dummy row. A multi-partition scheduler occurrence may issue one actual loader transaction per changed partition; each has its own durable receipt/ACK.
3. The admission witness binds the partition observation, actual driver, Arrow bytes (including legal schema-only zero-row streams), schema/type, epoch and declared replacement scope. Source-mutation-pending or schema fingerprint changes refuse before target data/ACK; they preserve old applied state and expose DELETES_LAGGED or SCHEMA_CHANGED.
4. Loader holds the real same-target jobs claim and exclusive target fence. In one target transaction it removes the previously materialized rows for **that partition only**, inserts the complete new partition rows with exact normalization, replaces its key/signature sidecar, updates quality/freshness/source checkpoint and writes the apply receipt. Keys must be derived/validated against the real typed batch, not arbitrary caller tombstones. A key moved to another partition or conflicting partition ownership must be explicitly handled or refused, never silently overwrite another partition.
5. The continuation codec is a structured partition signature/epoch map. Its signatures are opaque observations; only equality and exact previously acknowledged identity are allowed, with source generation/parent CAS for publication. No numeric global ordering or CDC offset is invented. A changed table UUID is POSITION_EXPIRED and requires the original explicit fresh-generation resnapshot path.
6. 74 advances the acknowledged partition map only after authoritative trusted Loader output and actual `jobs.complete_success` CAS; source capture or queued request is not applied. On crash, reclaim the same saved request and exact partition receipt; target-committed/control-not-complete replay must complete once. No failure/expired owner advances any partition ACK.
7. Detect/pull/reconcile periods are actual operator configuration (initial detect/pull3s, update7s). Inflight manifests are immutable and no overlap occurs. Reconciliation is separately declared C5 with DELETES_LAGGED boundary; it cannot relabel C4 results. Pausing a fixture DBA window requires no active occurrence.

## Actual acceptance window to execute

Use original twelve columns and three actual source partitions: baseline202401/202403; insert104 creates202402; update101 in202403; physical delete102 in202401. Prove pending mutation refusal, completed mutation replacement, unchanged replay, deleted key absence after catch-up, no stale resurrection, only changed-partition writes with unaffected cells unchanged, empty/disappeared partition, source epoch/schema failure, real 3s→7s reconfiguration and bounded source audit. Stop fixture writes, independently compare every typed cell in the entire native target (including untouched partitions) to source/known fixture truth, diff0. Then continue same data/semantic version through actual S0–S10 YAML maintenance, reviewed Pack, Gateway/CLI and autonomous dialogue. Until these exits actually run, C4/full-source acceptance is false.
