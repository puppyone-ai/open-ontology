# Oracle -> shared Loader / job-worker request

Recipient: root / MySQL shared-pipeline owner. Oracle owner does not edit shared
`lib.rs`, Cargo roots, pipeline, job-worker, semantic or evaluator files.
This request does not mark L3 or any Goal complete.

## Implemented producer, not a target publication

- Native module: `product/crates/oo-connectors/src/oracle/`.
- Product binary: root has now exported optional `oracle` and built
  `oo-oracle-source` in the main crate. The original `oracle-chunk-reader`
  `#[path]` harness is historical preflight evidence, not the product entry.
  See `ROOT-ORACLE-FEATURE-REQUEST.md` for root's fixed hash and dependency checks.
- Orchestration: `bounded_source.py`. `capture()` produces immutable per-chunk
  JSON parts, independent native typed verification, a transactional disk
  journal and `oo.oracle.sealed-snapshot/v1`.
- `c5_diff()` consumes two caller-pinned manifest SHA256s, rechecks journal and
  part SHA256s, and streams `changes.jsonl` plus `oo.oracle.c5-diff/v1`.
- C1 is separate: `c1_debezium.py` consumes Debezium Oracle envelopes, preserves
  transport/typed schema/transaction order, rejects conflicts/drift, and exposes
  complete transactions only after an END with contiguous expected row counts.
  It is not a redo parser or a target applier.

## Fixed snapshot/part shape

Binding: `source_id`, database identity (DBID + RESETLOGS_CHANGE# + PDB GUID +
PDB name + DB_UNIQUE_NAME + reader), registered owner, whole visible inventory,
explicit exclusions/reasons, pinned SCN decimal string, exact driver SHA256,
chunk row/byte limits and independent-verification policy.

Each `oo.oracle.bounded-chunk/v1` part has:

- `table`: owner/name, object ID/DDL stamp, every column's Oracle type,
  precision/scale/byte length/nullability/collation, validated composite PK.
- `lower_exclusive`, `upper_inclusive`: ordered typed PK vectors. Values are
  strings with the column's source type; NULL keys are refused.
- `rows`: ordered cell arrays aligned with `table.columns`, exact strings/nulls,
  **not** a single opaque JSON row destined for a text column.
- `row_count`, `logical_bytes`, `eof`, `scn`, `max_fetch_rows=1`,
  `reader_mutations=0`. Hashes cover exact file bytes.
- A separate manifest part entry binds ordinal, filename, SHA256, bounds,
  row count, EOF and independently verified cell count.

Mappings: NUMBER is canonical decimal text (including unconstrained NUMBER and
negative scale), never f64; RAW is exact hex; text preserves trailing spaces and
NULL (Oracle empty string is NULL); DATE is second-precision civil time;
TIMESTAMP retains 9 fractional digits; TSTZ retains local civil time plus signed
numeric offset (region ID is explicitly normalized, not claimed preserved).
LOB/LONG/unknown types, local-time-zone timestamps, nonbinary/nullable/unsupported
keys and no-PK tables are rejected or explicitly scope-excluded; never truncated.
Consumer must use approved exact typed target codecs, not guess coercions.

Requested shared API (names are proposal; payloads above are fixed):

1. `begin_snapshot(binding, inventory_digest) -> target_run_id`.
2. `apply_snapshot_part(target_run_id, ordinal, part_sha256, schema, bounds,
   typed_rows) -> durable_part_ack`. Data write, idempotency key and ACK must be
   in one target transaction. Same identity/ordinal with different bytes fails.
3. `finish_snapshot(target_run_id, sealed_manifest_sha256, inventory_dispositions)
   -> generation_receipt`. Verify all ACKs and EOFs before complete-generation
   publication. Excluded tables remain visibly incomplete.
4. `apply_c5_generation(parent_manifest_sha256, current_manifest_sha256,
   changes_sha256) -> atomic_generation_ack`. Refuse stale parent, identity/schema
   drift and partial snapshots. Nochange must not manufacture a new generation.

Current local journal ACK means **durable staging only**. Source ACK, target
apply/publication are false. C5 delete detection is full admitted-key
reconciliation, not an insert/update watermark or a C1 claim.

## C1 boundary and ACK

Use Debezium's mature Oracle LogMiner connector, with source-specific schema
history and opaque offset store. Preserve DB identity, connector/JDBC versions,
snapshot SCN, transaction ID, total order, commit SCN and row schema digest.
Do not create a Debezium offset from `current_scn` or silently start at "now".

Requested `apply_log_transaction(binding, transaction_id, commit_scn,
ordered_typed_events, offset_before, offset_after) -> atomic_target_ack`.
Checkpoint advancement requires complete transaction + target commit. HTTP 200
from the local spool only acknowledges durable capture, not this target ACK.
Replayed identical events are idempotent; conflicting replay, missing commit
position/END/rows, unsupported DDL or source reset requires explicit recovery.
Missing redo/expired snapshot must block and require retained-log recovery or an
explicit fresh baseline, never convert into C5 or claim SCN-only CDC.

A gap-free shared chunk->log handoff is still required: establish and retain the
connector log coverage at/before the pinned snapshot boundary, preserve open
transactions, and prove no gap/double application. The local Debezium initial
snapshot test is not proof of that shared bounded-chunk handoff.

## Semantic / question gates (2026-10-27 coordinator update)

Read `evidence/convergence-20261027/{semantics,agent}/HANDOFF.md`.
Their deterministic tests are not Oracle business/LLM receipts.

- A new Oracle source needs its own real S5 and business dictionary/provenance,
  current S0-S4 observation and independent business gold. No MySQL provider
  receipt may be copied or relabelled.
- Real local `semantic_scope` must bind the actual project, role, principal and
  `permissions_sha256` before cross-generation semantic reuse. This producer
  deliberately does not invent those fields.
- Then run full S0-S10, actual review/required-asset gates, regular YAML/Pack
  roundtrip, real publication and exact installed Skill/CLI/manifest-pinned
  questions. No model calls are authorized in this Oracle implementation run.
- Root/owners must refresh script pins through the official route. No live
  legacy selfbuild, standby PG, provider config or installed files are changed.
