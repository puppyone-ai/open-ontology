# PostgreSQL lane D adapter tests

Card D-S0S1: S0-D-01 / S1-D-01. Run from the repository root:

```sh
cargo test -p oo-connectors --lib --locked
python3 testing/engines/postgres/run.py --versions 13,14,15,16,17,18 --run-id <new-id>
```

The runner uses patch tags 13.23, 14.24, 15.19, 16.15, 17.11 and 18.6,
all `-bookworm`. Images must already be available locally. Each run resolves
and runs the local immutable image ID and records its registry digest and
architecture. It creates one `oo.lane=d` container at a time with tmpfs data,
1 GiB memory and two CPUs, then removes exactly that container in `finally`.
No existing services, volumes or images are changed. Runtime passwords and
TLS keys exist only in the ignored run's `private/` directory (0700).
The runner requires Docker and OpenSSL. It uses a package-local test-profile
optimization override without changing any Cargo manifest or lockfile.

`fixture.sql` is executed by an independent administrator, never by product
code. It includes a 2,000,000-row bigint-PK table, non-C nondeterministic ICU
text keys, composite keys, a NOT NULL unique-only key, no key, and growth past
a prior chunk budget. The reader has SELECT, REPLICATION and pg_read_all_stats,
not business-write grants. Publication creation is administrator-only.

## What this proves

- T0: actual version and capabilities, plus missing SELECT/REPLICATION/
  publication and DBA statements.
- T1 adapter boundary: a concurrent admin writer performs inserts, updates,
  deletes and key changes while the adapter reads one exported snapshot.
  Every table's Arrow rows are independently canonicalized and compared to
  source row counts and two SHA-256-derived signed 64-bit sums. Each receipt's
  complete IPC SHA-256 and byte count are independently recomputed.
- Composite row comparisons and physical TID ranges are EXPLAIN-checked.
  PG13 uses SingleRead without a key; PG14+ uses physical ranges.
- Closing the exporter invalidates physical reads. Stale revisions and
  over-budget reads fail instead of producing success receipts.
- T8 adapter boundary: sampled reader connection peak <= 1; measured rest is
  at least three times measured read work; active-session pressure prevents
  dispatch and eventually fails with an explicit budget error.

This is **not** source-to-Agent-DB/pipeline acceptance, a C1 handoff, a public
query, or a database support-matrix upgrade. Target write amplification is not
measured; no model is called. No standby/failover server is exercised here.
The source snapshot is an explicit coordinator-owned connection in addition
to each read task's single connection, not a claim of zero xmin retention.

## Product lifecycle

`postgres::PostgresEngine` wraps the existing verified-TLS `PgConnector` and
`PgSession`, catalog and type decoder. It uses read-only repeatable-read
transactions and bounded server cursor pages, preserving binary frame limits.
The old root public APIs and their admission gates are unchanged.

Call `plan_chunks` before `read_chunk`/`digest_range`. Every read must match an
issued plan. Bounds are bound parameters cast to source column types; source
collation, not Rust string order, determines membership. No-key physical plans
require `with_snapshot(token)` with a still-live external exporter. Never
persist ctid chunks as restart/cross-cycle checkpoints. Drop that engine and
replan after closing the exporter. PostgreSQL enforces snapshot validity on
every short transaction import. A new keyset cycle without an explicit
snapshot has per-chunk consistency, **not** cross-table/source-real-time
consistency; S2 log reconciliation is still required for that mode.

If a chunk grows past its row/byte budget, the adapter returns
`chunk_rows_replan`/`chunk_bytes_replan`; callers must discard all staged batches
from the failed chunk and replan with an appropriate smaller target. This card
does not implement a scheduler's adaptive retry loop. SingleRead on PG12/13
can therefore fail explicitly for a large no-key table. The sink is staging,
not an atomic publication API: publish only after a successful receipt.

The adapter refuses RLS, ordinary inheritance parents, empty/unsupported
column plans and schema drift. Non-default key comparisons not using the
EXPLAIN-verified composite index fail closed. Digest framing is
`N` for NULL and `V<byte-length>:<PG-text>` for non-NULL cells, separated by `|`,
with UTC/ISO/hex output settings and order-independent sums; it is not the
Arrow IPC receipt hash. C1 is only prerequisite-graded; this card does not
implement ChangeSource or create slots/publications.

## D-S2 extension

`postgres::changes::PostgresChanges` now provides read-only pgoutput capture;
`postgres::reconcile::C5Cycle` drives four-layer digest inspection through
`PostgresEngine::inspect_c5`. See `S2.md` for supported cases and explicit
acceptance limits. Run the separate small adversarial fixture with:

```sh
python3 testing/engines/postgres/run.py --suite changes --versions 13,14,15,16,17,18 --run-id <new-id>
```

Start capture before reading snapshot chunks. Persist complete transactions
before any external durable-owner ACK; this reader never advances a slot.
`next_batch` returns complete transactions with one image per change, including
key-only delete images. Unsupported messages and unchanged TOAST are refusals,
not silently omitted data. `required_wal_headroom` is a measured byte budget,
not proof that a particular number of hours remains.

For C5, persist the returned source digests only after every requested payload
read and target application succeeds. An empty `read_chunks` suppresses payload
work; the target coordinator must also suppress rewrites and new generations.
Timestamp hints are positive evidence supplied by the caller, never cursors.
Recent ranges do not limit digest coverage. C5 remains range reconciliation
with per-read snapshots, not transactionally consistent CDC.
