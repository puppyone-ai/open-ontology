# Current PostgreSQL source lane

The subsequent real shared-target/normal-worker-ACK delivery is documented in
[CURRENT-TARGET.md](CURRENT-TARGET.md). The original source artifacts are intact;
their source slot has now been ACKed by the normal worker. Do not reset it or
rerun the original DML.

This upgrades the existing `postgres-preview` executable and `connection_cycle.py`.
`src/receipt/current.rs` reuses the original receipt column planner, typed Arrow
encoder and `capture.rs` pgoutput decoder. No new ingestion executable, dependency,
root Cargo change, Gateway integration, model call, or source-ACK implementation.

## Offline CLI

Every command below exits successfully without credentials, Docker, a source,
model, identity service, old fixture, or current working directory dependency:

```sh
postgres-preview --help
postgres-preview capture-receipt --help
python3 tools/postgres-preview/connection_cycle.py --help
python3 tools/postgres-preview/producer_v2.py --help
python3 tools/postgres-preview/whole.py source --help
python3 tools/postgres-preview/linux/native_transaction_adapter.py --help
python3 tools/postgres-preview/linux/native_transaction_producer_runtime.py --help
```

Build this existing independent tool, not the root workspace:

```sh
cargo build --locked --offline --manifest-path tools/postgres-preview/Cargo.toml
```

Use `examples/current.example.json`. Replace the actual source system identifier,
database OID, owned slot/publication, endpoint and approved columns. There are NO
model, DBA, target, shard, fixed identity, or total-row-budget fields in this
configuration. Unknown keys fail. DBA setup is separate: a PG17 logical source,
existing publication and `pgoutput` slot, a non-superuser REPLICATION reader with
only schema USAGE, approved column SELECT and `pg_control_system()` EXECUTE.
TLS verification is mandatory. The product never creates these source objects.

For example, with an exact pinned executable:

```sh
BIN=/absolute/postgres-preview
SHA=$(shasum -a 256 "$BIN" | cut -d' ' -f1)
python3 tools/postgres-preview/connection_cycle.py preflight \
  --config /absolute/source.json --binary "$BIN" --binary-sha256 "$SHA" --out /absolute/runs/preflight
python3 tools/postgres-preview/connection_cycle.py baseline \
  --config /absolute/source.json --binary "$BIN" --binary-sha256 "$SHA" --out /absolute/runs/baseline
MANIFEST=/absolute/runs/baseline/capture/manifest.json
MSHA=$(shasum -a 256 "$MANIFEST" | cut -d' ' -f1)
python3 tools/postgres-preview/connection_cycle.py continuous \
  --config /absolute/source.json --binary "$BIN" --binary-sha256 "$SHA" \
  --resume "$MANIFEST" --resume-sha256 "$MSHA" --cycles 3 --interval-seconds 1 \
  --out /absolute/runs/continuous
```

`cycle` captures once. `continuous` is a bounded number of source capture cycles,
NOT a target convergence daemon. Each invocation uses a new output directory.
`capture-progress.json` means captured, never applied or acknowledged. A failed
attempt is retained; resume from the last COMPLETE pinned manifest into a new
directory. Do not overwrite failed directories or recreate the slot.

## Guarantees and explicit limits

- Preflight reads metadata only: PG17, real cluster/database identity, replication
  privilege without business-write grants, exact publication membership, supported
  columns, ordinary tables, no RLS/filter/column-publication surprises, inactive
  durable healthy slot. Current support requires a single bigint primary key.
- Baseline runs under one read-only repeatable-read transaction. The original
  exported snapshot token, observed WAL position and original slot confirmed LSN
  remain distinct. Arrow streams are chunked; all rows and empty tables appear in
  the manifest. A snapshot token is an observation, not a still-open snapshot.
- `chunk_rows`/`chunk_bytes` bound chunks, not total rows or transaction size.
  A single oversized cell/message, timeout, schema drift, unsupported type or
  TRUNCATE fails explicitly. No clipping or fallback to a fixture/sampled path.
- CDC streams `pg_logical_slot_peek_binary_changes` with NULL message count,
  through an observed WAL ceiling. Complete original BEGIN/COMMIT/end LSN/xid,
  ordered messages, old keys and unchanged-TOAST markers are retained. There is
  no current-row reread that could substitute a later transaction's values.
- Each raw part is hex pgoutput NDJSON. `relation_messages` retains original
  relation messages needed to decode a resumed spool even if PostgreSQL emitted
  them only in an older, skipped transaction. Raw column bytes remain sensitive.
- Resume verifies the pinned manifest, part sizes/digests and source/catalog
  identity. A source slot ahead of captured progress fails. The producer does
  not advance, drop or recreate a slot; it checks the confirmed LSN is unchanged.
- The client row/payload buffers are chunked. The manifest's part/transaction
  metadata grows with the capture; PostgreSQL decoding work and retained WAL
  still require operator resource management. An unacknowledged prefix is
  decoded again on resume. This is not a production long-running capacity claim.
- Baseline/log handoff is `reconcile_from_original_slot_required`, NOT an atomic
  slot-exported-snapshot handoff. Concurrent-baseline reconciliation, retention
  loss, failover/timelines, source pressure and broad type coverage are not live
  acceptance claims from the owned tests.

## Original source handoff boundary (historical)

`oo.postgres-preview.capture-set/v1` is a source observation. It is deliberately
NOT an admission receipt or `oo.jobs.native-transaction-producer/v1`.

At the original source-only handoff, the shared loader rejected these manifests (`protocol` for the
chunked C1 baseline and `closed` for native transaction import in the pinned
probe). The legacy PG worker group branch verifies target attestation and commits
its job/watermark, but its current path has no PG slot `pending_acks`/drain branch.
The historical October fixture's frozen successor worker is not current evidence.

Main-owned interface work, without weakening admission/lease guards:

1. Admit the pinned manifest plus every Arrow/WAL part, original relation messages,
   source identity, privileges, schema and complete transaction boundaries.
2. Stage baseline and ordered CDC in the existing private C1 path. Reconcile from
   the ORIGINAL slot boundary with per-key CDC/tombstone precedence; never turn
   chunks into invented source generations or equate observed WAL to snapshot cut.
3. Decode raw pgoutput with old-key and unchanged-TOAST semantics. Publish an
   entire original transaction atomically, not one file/chunk at a time.
4. After normal target attestation and job CAS, persist a pending ACK naming the
   exact source identity/slot and original COMMIT **end_lsn**. Normal worker
   recovery must perform/verify that ACK idempotently, including crash between
   target commit and source ACK. The source tool must not write an ACK receipt or
   accept a caller's `acknowledged=true` as target authority.

The generalized native-page adapter still accepts independently admitted legacy
pages with configured objects/source/slot, positive occurrence pins and a pinned
normal `native-state` export. It chooses the earliest unapplied commit, never the
latest one. It refuses current capture-set manifests instead of fabricating pages.
The runtime emitter accepts configured producer path/SHA and checks the actual
worker binding/pin/source/generation; it never advances the source.

That source-only handoff changed no shared code/SQL and claimed no normal worker
ACK. The subsequent PG-only shared integration uses immutable SQL0116/0117 and
the normal pending-ACK queue; see CURRENT-TARGET.md for actual tested boundaries.
Gateway/Skill/full-PG publication remains outside this technical delivery.

## Reproducible owned tests

`testing/convergence/postgres-current/owned_source.py` checks disk/containers,
pins the locally available PG17 image and creates one uniquely named, resource-
bounded isolated source. It never recreates/restarts the main target.
`exercise.py` verifies a >100k-row baseline and real repeated I/U/D using an
independent full-row SQL/Arrow/pgoutput oracle, including >10k updates, a primary
key change, unchanged TOAST, multiple pending transactions and same-slot replay.
`verify_boundaries.py` tests refusal paths and continuous empty polling.
`target_contract.py` creates an empty owned target database and records exact
shared loader refusals, without installing SQL, making leases or faking receipts.

The independent Python oracle is test-only, not a second product ingest/target.
Its in-memory replay result is never passed to the worker as an apply receipt.
