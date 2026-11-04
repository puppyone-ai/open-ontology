# oo-load (local SourceEngine admission)

Build without changing the root workspace lock:

```sh
CARGO_TARGET_DIR=target cargo build --manifest-path tools/oo-load/Cargo.toml --locked
target/debug/oo-load --stack stack.yaml --source erp \
  --tables public.orders,public.customers --target target.json \
  --secrets-dir evidence/my-run/.private/secrets
```

Uses the existing `oo.stack/v1` JSON/YAML parser and owner-private SecretStore.
Specify an explicit `postgres`, `mysql`, or `oracle` engine. Selected tables
must be qualified and inside stack scope. Wildcard discovery, SSH and automatic
engine detection are not implemented. PostgreSQL/MySQL require `tls_ca`.
Oracle uses its existing connector transport; this entry does not add TLS.
For 11g, `--oracle-worker EXECUTABLE` selects the existing trusted thick-worker
protocol; the executable is operator-controlled, receives credentials on stdin,
and must not log its input.

Target JSON (configuration only; no inline secrets):

```json
{
  "host": "agent-db.internal",
  "port": 5432,
  "database": "agent",
  "user": "loader",
  "password": {"secret_ref": "agent-password"},
  "tls_ca": {"secret_ref": "agent-ca"},
  "run_id": "snapshot-001"
}
```

`local_fixture:true` permits a target without TLS only on literal loopback.
The target connection must be authorized to create schemas, tables and views.
Do not point this experimental entry at an existing customer database.

## Admission and recovery

Every chunk passes identity/revision, row/byte-budget and canonical IPC SHA-256
validation before typed COPY. Empty chunks must carry a schema. Oracle flagged
empty strings become NULL after the original IPC has been verified.
Bounds are opaque: the client checks adjacency, not source collation order.
Budget failure discards the partial sink and asks `SourceEngine::split_chunk`
to replace only the failed source range. MySQL and PostgreSQL keyset plans
support this hook; children must cover the exact parent bounds, have newer
revisions and preserve plan-wide unique IDs. Other chunks keep their revisions.
An unsupported hook falls back to a whole-table replan at half the row budget.
There are at most 16 attempts. Each replacement still starts a new private target
stage and rereads the resulting plan; this is not immutable-parent composition.

Target-only namespace `_oo_load` keeps private storage, original per-chunk
receipts (including watermarks), complete plans and logical tuple write bytes.
The existing pipeline COPY cell encoder is reused. Schema drift fails closed.
A dedicated session advisory lock fences writers for each source/table.
Chunk data, its receipt and its meter commit together; a complete generation
atomically replaces a view in `oo_load_data`. Published tables are immutable.
Older generations and abandoned plans are retained; no automatic garbage
collection is performed.

Reusing a completed `run_id` returns durable receipts without reading chunks or
rewriting target rows. Use a new `run_id` for refresh/correction. A pending run
replans and rereads; only identical receipts skip COPY. Physical plans may be
recaptured and are never treated as durable source snapshots.
Use the same source ID only for the same source endpoint and table namespace.

Output is one JSON line per completed table, including its target view name,
receipts, the effective ReadBudget and write bytes. The meter counts committed logical stage tuple bytes,
including overwritten chunks and abandoned plans of the same run; it is not
WAL/index/TOAST/physical I/O. Atomic publication changes metadata only.
Error messages are closed codes; no SQL, credentials or driver errors are echoed.

## Acceptance boundary

The snapshot entry point is a local workaround, **not** integration with the legacy C5 admission,
jobs lease, immutable-parent overlays, release coordinator or Gateway.
It does not claim CDC, chunk-window correction, transactional cross-table
consistency, or completed S1-M-01 acceptance. Its local view publication is
explicitly `gateway_published:false`. Until correction and comparison prove
otherwise, output is `consistency:B` and `source_time:unmeasured`.
Supported Arrow physical types are closed in `engine_load_pg::column`;
unsupported types fail, never stringify silently. Oracle's canonical text
columns remain text, as declared by its engine; receipt hashes retain metadata.
Unsigned 64-bit values widen to PostgreSQL numeric without signed overflow or
floating-point conversion; the dedicated target regression includes `u64::MAX`.

Tests:

```sh
cargo test -p oo-pipeline --locked
CARGO_TARGET_DIR=target cargo test --manifest-path tools/oo-load/Cargo.toml --locked
```

The real PostgreSQL target test is explicitly ignored unless run with
`OO_LOAD_TEST_TARGET` pointing at an owner-private synthetic config containing
`port` and `password_file`. A default ignored test is not acceptance evidence.

Run both dedicated-target regressions and actual process-kill recovery with:

```sh
python3 tools/oo-load/test-target.py --run-id m-load-recovery
```

This starts one fresh lane-M-labelled PostgreSQL target, tests superseded-run
refusal and empty replacement, then SIGKILLs real test processes after chunk
commit and after publication commit. A final resume must preserve exactly one
receipt, three fixture rows and unchanged metered bytes. Only its own container
and generated secrets are removed. Do not run alongside a two-database matrix.

## Incremental local target (M-S2, partial)

PostgreSQL has two explicit incremental entry points, using the same `--stack`,
`--source`, `--target` and `--secrets-dir` arguments as the snapshot loader:

```sh
oo-load --stack stack.json --source fixture --target target.json --sync sync.json
oo-load --stack stack.json --source fixture --target target.json --c5 c5.json
```

`sync.json`:

```json
{
  "initial_position": {"kind": "pg_lsn", "lsn": 123456},
  "baseline_aligned": true,
  "slot": "oo_slot",
  "publication": "oo_publication",
  "required_wal_headroom": 134217728,
  "max_batches": 100
}
```

The position is an example, not a usable starting position. The caller must
already have a baseline aligned to that recorded position. The loader does
**not** infer alignment from a successful snapshot or implement C5-to-C1
handoff. It rejects a C5 parent with `sync_c5_handoff_required`.

C1 commits each source batch across its selected tables in one target
transaction. Typed delta rows and durable tombstones overlay the immutable
snapshot; unchanged parent rows are not copied. The target position, generation
counter and logical tuple-byte meter commit in that transaction. A retry resumes
from the target position, ignoring the initial position in the configuration.
Only after commit does the coordinator advance the configured replication slot.
Restart also repairs feedback lost after a committed target transaction.

The DBA must create the publication/slot and grant the reader REPLICATION,
read access, and EXECUTE on `pg_control_system()` and
`pg_control_checkpoint()`. The reader must not own or have write privileges on
source tables. Slot advancement is replication metadata feedback, not business
DML. The control connection requests read-only transactions. Source controls,
including lineage/retention queries, are paced; do not disable source budgets.

Lineage records system identifier, timeline, database and relation OIDs, slot
and publication. Changed lineage/scope/baseline, expired history, future
positions and non-monotonic batches fail explicitly. This is not a failover
acceptance claim: the shared ChangeBatch contract does not carry an authenticated
source epoch. Use a direct, stable instance endpoint.

`c5.json`:

```json
{
  "cycle": 1,
  "full_read_every": 10,
  "positive_hints": [],
  "recent_chunks": []
}
```

The scheduler supplies the cycle number and optional positive time/recent-window
hints; NULL/absent hints cannot exclude ranges. PostgreSQL's C5 inspection checks
all source range digests, with periodic full reads. Replacements are bracketed
by digest checks; concurrent drift returns `c5_range_changed_retry` without
confirming the new source baseline. Retry under a new cycle run ID after a
successful publication, or the same ID after a failed attempt.

Use a new target `run_id` for each C5 attempt. Unchanged parent ranges remain
referenced, and complete replacement ranges are staged before atomic view
replacement. An established no-change cycle does not update target metadata,
write row payload or create a generation. Initial source-digest/lineage
checkpoint creation does write metadata even when baseline rows match.
Changed range plans/lineage require explicit rebaseline rather than a silent
whole-table refresh. C1 and C5 cannot operate simultaneously on one binding.

Both paths publish **local table views only**, not Gateway generations.
Byte meters count committed logical staged/delta/tombstone tuples, excluding
indexes, WAL and control metadata. The test runner separately measures total
target relation bytes across no-change cycles.

MySQL and Oracle use the same durable C5 checkpoint boundary. Oracle C5 never
starts LogMiner. MySQL persists server UUID; Oracle persists DBID/resetlogs.
The coordinator rechecks these before confirming a cycle. Numeric file cursors
are used for MySQL C1, even when the server enables GTIDs; this loader does not
compare GTID sets. Unsupported modes fail rather than falling back to snapshots.

Build MySQL C1 explicitly (it is not in the default artifact):

```sh
cargo build --manifest-path tools/oo-load/Cargo.toml --locked --features mysql-binlog
```

MySQL `--sync` configuration:

```json
{
  "initial_position": {"kind":"mysql_file_pos","file":"bin.000001","position":4},
  "source_epoch": "server-uuid-from-the-aligned-baseline",
  "baseline_aligned": true,
  "max_batches": 100,
  "server_id": 42,
  "longest_cycle_seconds": 28800,
  "recovery_seconds": 3600
}
```

The initial epoch/position must belong to an already aligned baseline, not
the time this command is invoked. Retention is checked against actual binlog
history. The client server ID must be unique and distinct from the server.
Prefer the repair handoff below over a manual alignment assertion.

Oracle `--sync` additionally requires `oracle_logminer_opt_in: true`,
`initial_position: {"kind":"oracle_scn","scn":...}` and a `mining` object with
`service`, `user`, and `password.secret_ref`. Use a common user on the matching
root service for CDB sources, and the thick worker for 11g. D11a applies:
Oracle may write its own SYS/SYSTEM LogMiner session objects internally; this
is not the default C5 path and is not a source-wide zero-write claim.
The current engine rejects SCNs preceding its new reader session with
`logminer_session_rebaseline`. The one-shot CLI cannot yet resume such an
Oracle C1 session safely; do not bypass this guard or replace the cursor with
the current SCN. C1 integration is not Oracle T2 acceptance.

### MySQL C5-to-C1 Handoff

`--handoff FILE` uses the complete explicit table scopes of all listed stack
sources. `--source` must name the first member; `--tables` overrides are refused.
All members share this invocation's target Agent DB.

```json
{
  "sources": [
    {"source":"first_source","server_id":42},
    {"source":"second_source","server_id":43}
  ],
  "max_keys":100000,
  "max_bytes":33554432,
  "required_history_seconds":32400
}
```

Subscribe before reading, repair every member through its L_end with log
priority/tombstones, then atomically publish all baselines and durable C1
positions in one outer target transaction. A failed member rolls back the
whole switch. Epoch and retained-file boundaries are rechecked before commit.
The output positions/epochs can seed subsequent `--sync` configurations.
Use a new target run ID; a previously successful handoff is not replayed as
another snapshot. Resume via C1 instead.

This bounded prototype admits up to 16 sources and 100,000 keys / 32 MiB
repair state per source; each final table must also fit the supplied chunk
budget. Exceeding a budget fails explicitly, without partial publication.
It is not an unbounded disk-spooled bootstrap or cross-source snapshot claim.
Only local Agent DB views are published.

Validation entry points (all create only labelled, owned fixtures):

```sh
python3 tools/oo-load/test-sync-target.py --run-id my-sync-target-run
tools/oo-test/oo-test t2 --versions 13,14,15,16,17,18 --run-id my-pg-t2-run
```

The PostgreSQL T2 runner uses one database container at a time, with separate
source and Agent databases and separate reader/admin connections. Its compact
fixture exercises inserts, updates, deletes, key changes and cross-table source
transactions. It is not full type/scale coverage or a T7 proof.
