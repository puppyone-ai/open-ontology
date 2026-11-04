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
Budget failure discards the partial sink and replans the entire table at half
the row budget (at most 16 attempts), never mixes old and new plan revisions.
This conservative fallback replaces an absent shared growth-split interface.

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
receipts and write bytes. The meter counts committed logical stage tuple bytes,
including overwritten chunks and abandoned plans of the same run; it is not
WAL/index/TOAST/physical I/O. Atomic publication changes metadata only.
Error messages are closed codes; no SQL, credentials or driver errors are echoed.

## Acceptance boundary

This is a local workaround, **not** integration with the legacy C5 admission,
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
