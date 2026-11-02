# Captured PostgreSQL -> shared typed target -> normal worker ACK

## Delivered boundary

This mode consumes the ORIGINAL current capture artifacts. It does not recapture,
write business rows on the source, create a new ingestion service, reset a slot,
or provide an operator ACK command.

The existing native worker selects `c1_baseline_format: "pg-capture/v1"` with
`loader.args: ["import-native-transaction"]`. The existing PG CLI's
`current-worker-producer --config FILE` emits references bound to the worker
binding, pin and initial generation. `registration.registry_path` pins the bundle
reference. A normal `native-loader-put` may replace the loader/admission executable,
but cannot rewrite the source, producer arguments, registry or occurrence request.

`oo.postgres-preview.target-bundle/v1` names SHA-pinned baseline, transaction
capture, both original configurations, and both original driver executables.
The PG-only admission handler verifies those files, every part, approved columns,
source identity, full table membership (including empty tables), exact raw
BEGIN/COMMIT/xid/end-LSN/count/table scopes, and the snapshot/log boundary.
The loader pins the admission executable in code, reruns admission, independently
decodes every baseline Arrow part, and checks all keys before its first connection.
A caller-supplied `trusted` field is refused.

## Existing target machinery

- Baseline uses `c1_typed::copy_bytes`, existing `_oo.c1_typed_*` tables/functions,
  bounded COPY and real `jobs.loader_hold` leases. Each chunk is checkpointed;
  no arbitrary total-row limit is introduced.
- Raw pgoutput transactions are replayed in original order, each inside ONE
  target transaction. Old primary keys and unchanged TOAST values are retained.
  An unchanged value comes from the preceding target row, never a current source
  reread. All raw part/scopes are revalidated even on replay.
- SQL0116 adds PG-specific provenance and transaction records plus the additive
  PG engine guard. Non-PG calls delegate to the exact prior guard.
- SQL0117 corrects ONLY seal ordering: mark the typed format sealed before the
  baseline run changes state, preserving the existing 0042 guard. SQL0116 remains
  immutable, including its retained first failure.
- `_oo.pg_current_run.result` is a durable **typed-stage** proof (`staged`), not a
  fabricated legacy PG group/full-import attestation. The normal worker verifies
  it and every original transaction count, then commits job success and a pending
  source ACK together. The source fence is retained until ACK completion.
- Normal `native-step` drains that pending ACK: rechecks target proof, successful
  job CAS, pinned loader, original source configuration, cluster/database/slot/
  publication identity and exact boundary, then advances the original slot.
  The result must equal the original final COMMIT end LSN.
- If the source already confirms that exact end LSN after a process loss, the
  worker skips advancement and commits the control ACK idempotently. Any other
  unexpected source boundary refuses. No source read receipt is called an ACK.

Use the normal installers (`install-native-update`, jobs deploy, `native-install`,
then `install-native-c1-typed` with `source_engine: "postgresql"`). The order matters:
0115 requires the normal worker schema. No direct migration markers, leases,
success rows, or receipts are inserted by the test harness.

## Actual owned verification

Evidence: coordinator `postgres-current-target-20261030`.

- Source: original `oo-pg-current-src-20261030-d3f1dae9`; no new DML or capture.
- Target: existing empty `pg_current_target_1793312085794163000`, bootstrapped by
  the normal installers. No shared PG restart or WAL setting change.
- 100,123 items + 17 notes + an empty table; 99 baseline COPY receipts.
- Four original transactions: 5 inserts, 12,019 updates, 5 deletes, including a
  primary-key update and unchanged TOAST. Each transaction commits atomically.
- Independent complete gold: 100,140 rows, 200,297 values, zero differences.
- A real process-group termination during transaction 3 retained only transactions
  1 and 2; the 12,011 item updates from transaction 3 were not partially visible.
  Normal lease reclaim continued the same task and original files.
- Actual SIGKILL after target seal + job CAS: one durable pending ACK, source
  confirmed position still `0/3F3F1E0`.
- Actual SIGKILL after real source advancement but before control commit:
  source confirmed `0/5F13128`, pending ACK still unconfirmed. A normal
  `native-step` then returned `complete=acknowledged ... replay=true`.
- Final task `44f2339a-990f-421a-89f6-1414ea0b0847`, generation 3, succeeded;
  source fence cleared. Transaction records and COPY receipt counts did not grow.
- Negative copies exercise corrupted last Arrow, rehashed corrupt WAL, incomplete
  commit, omitted empty table, wrong source identity/boundary and caller trust flags.
  Original artifacts and target remain unchanged.

## Capture/ACK handoff boundary

The original handoff above stopped at initial captured-scope target/ACK
acceptance. Its typed physical tables were private target state, not
Pack/Gateway-visible full-import mappings. The downstream bridge below consumes
that completed handoff without changing its receipts.

The current bounded bridge accepts single-bigint-key tables with bigint/text
columns. Other types fail explicitly. New live CDC cycles, concurrent snapshot
overlap, failover/retention loss, broad type/pressure matrices and Linux release
were not accepted here. Capture guards refuse ambiguous snapshot overlap rather
than relabeling a snapshot or deriving a fake LSN.

The source slot has now legitimately advanced. Do not rerun old source-capture
tests expecting the pre-ACK slot, reset it, or repeat the original fixture DML.
The source capture artifacts remain intact for independent replay/audit.

## Bounded downstream publication (2026-10-30)

Evidence: coordinator `postgres-current-publication-20261030/HANDOFF.json`.
This is one acknowledged initial cut, not completion of the full PG lane.

Migration `0118_native_pg_current_publication.sql` is additive and PG-specific;
0116/0117 remain byte-for-byte unchanged. Install only this bridge with:

```text
oo-pipeline install-native-c1-typed
{"source_engine":"postgresql","operation":"pg_current_publication","target":{...}}
```

The authenticated operator needs existing target administration and control
row-lock privileges. No jobs write privileges are granted to the target owner;
the runtime role cannot invoke publication. Invoke the existing typed CLI:

```json
{
  "protocol": "oo.pipeline.native-c1-typed-stage/v3",
  "operation": "pg_current_publish",
  "source_id": "pg_current_20261030_d3f1dae9",
  "task_id": "44f2339a-990f-421a-89f6-1414ea0b0847",
  "task_generation": 3,
  "binding_id": "21a7c82b-e5a5-4714-bfff-d0e31af0303d",
  "bundle_sha256": "b258c55061106e799167f4717f20193818528e9604bb75b1c970dfc3c043f49f",
  "end_lsn": "0/5F13128",
  "target": {"host": "...", "port": 25487, "database": "...", "runtime_principal": "...",
    "password_file": "...", "ca_file": "...", "ssl": "verify-full",
    "shard": "pg_current_owned", "owner": "f42df4dd-aaf0-417d-97ac-5cac5d77675b"}
}
```

Send the document to `oo-pipeline native-c1-typed-stage`. It accepts no caller
tables, fabricated receipt, trust flag, source generation or new lease.

- Authorization joins the original PG staged proof, catalog, transaction scopes,
  source instance, succeeded task/generation, occurrence, watermark, disabled
  binding, cleared source fence and completed worker ACK. A source connection is
  never opened. Binding/progress row locks serialize with normal scheduling.
- The actual export is **server-side typed INSERT SELECT**, not an Arrow file
  masquerading as an old capture. Each keyset page has at most 2,048 rows and
  4 MiB of serialized source-row evidence; an oversized row refuses. Values and
  per-row positions retain their PostgreSQL types. Page hashes include typed
  row keys, positions and NULL/empty distinctions. Empty tables are explicit.
- Existing physical mapping, target fencing, full-import shadow, batch, quality,
  mirror and sealing functions create the real downstream runs. Every table,
  publication receipt and freshness transition commits in one transaction.
  Failure rolls everything back; exact replay returns the existing cut.
- `_oo.pg_current_publication` retains append-only PG-specific provenance.
  No `_oo.native_transaction_group` or `_oo.native_c1_baseline_typed` attestation
  is manufactured, and the original task, ACK and staged result are unchanged.
- PG-only SQL dispatch allows the **unchanged** `native-library-prepare` and
  existing library publication precheck to validate this proof. Non-PG dispatch
  delegates to the exact preceding functions. No `native_library.rs` edit.
- The ordinary full-import installer grants INSERT on new shadows as usual;
  no consumer/Gateway read grants, Pack, semantic approvals or runtime pointers
  are changed. `source_committed_at` remains unmeasured; `reconcile_status=never`
  does not relabel the prior root gold as a new reconciliation.

Owned verification: three real published full-import runs, 100,140 exported
rows in 50 pages (largest 2,048 rows / 726,532 bytes), including the empty table;
exact replay; failure after the 100,123-row items table sealed leaves no import
effects; 19 rollback-scoped negative cases, a real control-lock timeout with
successful replay after release, an incomplete-library refusal, and 11 focused
PG Rust tests.
The existing Selfbuild mapping helpers bind all seven columns, and the real
`native-library-prepare` returns three datasets with `precheck=rolled_back`.
Six sampled exported rows match the stage; the root's complete 100,140-row /
200,297-cell gold and 140-artifact audit were referenced, not repeated.

The legacy capture-bound `catalog-import-native` is not relabeled or bypassed.
Main integration consumes the ordinary mapping vector from the prepared native
library receipt and the retained genuine PG catalog/provenance. Building or
activating a reviewed Pack, granting consumer access, Gateway acceptance,
new CDC cycles, other types and pressure/failover remain outside this delivery.
