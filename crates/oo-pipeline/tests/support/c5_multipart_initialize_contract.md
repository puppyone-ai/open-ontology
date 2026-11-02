# 0065 handoff: bounded multipart baseline

This migration does not enable a full-refresh fallback or publish data. It
creates topology from the current table's sealed multipart run, without reading
the data heap or inserting its keys into `c5_member`. 0058 remains unchanged.

## Interface

`_oo.c5_initialize_multipart_if_absent(shard text, owner uuid, fence bigint,
source text, schema text, table text, fingerprint text, topology bigint,
keys text[], expected_source_generation bigint) RETURNS jsonb`.

The JSON operation is `initialize_multipart_if_absent` in
`oo.pipeline.native-c5-state/v1`. It accepts the existing lease/target envelope,
`schema_fingerprint`, `chunk_topology_revision` (initially 1), `key_columns`,
and `expected_source_generation`. It rejects caller-supplied manifest, bounds,
chunks and chunk IDs. The fingerprint remains the caller's pinned source-schema
fingerprint, not a digest computed from current source rows. The independent
sealed schema/type digests and target key types/index are checked and exported;
the source adapter must check its pinned source-schema fingerprint before reads.

The installer registration belongs to main:

- version `native-c5-multipart-initialize/v1`
- file `0065_native_c5_multipart_initialize.sql`
- exact predecessor 0064; execute as `oo_target_owner`
- the state API checks that version's exact compiled SQL digest before use.

## Generation and digest contract

Let H be the source watermark and B the current table freshness generation.
H is a source-wide CAS token, not a physical table generation. B <= H is valid.
The initializer locks H before table freshness and C5 state, checks the supplied
H, and leaves both generations, all existing data and all receipts unchanged.

`c5_table.source_generation` remains the actual table generation B.
`c5_multipart_baseline` retains the original sealed run and B permanently.
An unrelated table update must never rewrite either to H.

For multipart state, export returns:

- `source_generation`: H, for source-wide CAS / next generation H+1
- `table_source_generation`: actual current `c5_table.source_generation`
- `table_physical_name`: current table freshness physical name
- `baseline`: immutable original run, generation, physical name, schema/type/
  config digests, ordered logical key columns and kinds
- `input_digest`: this table's actual input digest, NOT a source-wide digest
- each chunk's `baseline_membership_pending`.

Export fails closed if table freshness and C5 state disagree. Export is read-only.
0066 must compare H against the source watermark, separately compare the actual
table parent/physical generation, and use that parent for overlays. Guard,
after-seal guard, commit, observe, publication and worker input must agree on
these semantics. Do not retain the old `table_generation == H` assumption.

Initial chunk digests stay NULL. Qualified hydration independently computes a
source-compatible SHA row digest from verified archive cells (see below).
Original part Arrow digests are receipts, not the layered producer's row digest.
Never bootstrap by merely reading the current source.

## Topology and membership

Manifest arrays determine order, never PostgreSQL string sorting. Typed MySQL
`Int`, `Text` and `Bytes` bounds convert to the existing C5 representation.
Intervals are `(lo, hi]`. Only the first low and last high become JSON null;
every interior boundary is preserved exactly. Null outer edges cover inserts
below/above the captured extrema. Integers outside signed i64 are refused
because the current C5 key identity cannot represent them.

The table metadata is `_oo.c5_multipart_baseline`, keyed by
`(source,schema_name,table_name)`, with `run_id`, `source_generation`,
`physical_name`, `config_digest`, `schema_digest`, `type_digest`,
`key_columns`, `key_kinds`, `selected_columns`, `projection_sha256`.
Projection is computed from the baseline's stored physical mappings in physical
attribute order, not from caller-supplied columns. Export includes this ordered
`selected_columns` list.

`_oo.c5_chunk.baseline_membership_pending` defaults false for existing states;
new multipart chunks start true, membership revision 1, with zero eager member
inserts. Pending means UNKNOWN prior membership, never empty membership.
0065 wraps the old guard and after-seal guard to reject pending chunks.
The internal old functions are not executable by the runtime role.
0066 may replace those wrappers only when it implements the pending contract.

The retained-part hydration below clears pending atomically with membership and
its hydration proof. 0066 must require that bootstrap to be complete before
updating the table. No unchanged-path acknowledgement is allowed merely because
c5_member is empty. Full-scan completeness must not be claimed while any chunk
is pending.

## Retained-part hydration

The implemented `hydrate_multipart` operation accepts only the common
protocol/source/schema/table/target/lease envelope plus:

```json
{
  "operation": "hydrate_multipart",
  "expected_source_generation": 7,
  "chunk_id": "0",
  "arrow_path": "/private/retained/0.arrow"
}
```

Use one part per call. The path must be absolute, regular, non-symlink, private
(no group/other permissions), and at most 32 MiB. Rust fetches trusted metadata,
verifies the stored manifest's canonical chunks digest, verifies the entire
Arrow byte SHA against the sealed part, then uses the existing Arrow decoder.
Ordered field names, row count, ordered key columns, non-null keys, supported
types and unique typed identities must match. The API never accepts member keys,
expected Arrow SHA, a caller manifest, or boundary overrides.

SQL signatures:

```text
c5_multipart_hydration_manifest(shard, owner, fence, source, schema, table,
                              chunk text, expected_source_generation bigint)
c5_hydrate_multipart(shard, owner, fence, source, schema, table,
                    chunk text, expected_source_generation bigint,
                    arrow_sha256 text, members jsonb)
```

Both return JSON. Both recheck the current baseline B/physical and original
sealed run, part ACK, schema/type/config identity. The second is the trusted
Rust loader's typed-key sink, not a public JSON operation accepting arbitrary
members. SQL validates counts, key types/encoding, duplicate and cross-part
overlap, constructs Loader keys, and writes `c5_member` plus
`c5_multipart_hydration` atomically. Exact replay is allowed; conflicting replay
is refused. It does not advance scan cursors, source/table generations, apply
receipts or source observations.

Qualified hydration also recomputes `oo.c5.arrow-sha256-xor/v1` from the verified
archive bytes, never from a caller's proof/digest JSON. Qualification uses the
trusted baseline physical column types and Arrow field/nullability metadata:
signed integers, bool, text/bytes, numeric precision <=38 and scale <=30,
date32 and microsecond timestamps (UTC for timestamptz). Unsupported types
leave the digest null. The canonical row frame matches the qualified producer:
uppercase hex of each canonical cell plus its NULL flag, joined by `#`, SHA256,
XOR the first two u64 words. Null and empty strings have different flags.

`c5_seed_multipart_digest` rechecks the archive/hydration/projection and stores
the seed in the private hydration row plus `digest_receipt`. It does not set
`last_digest_scan_id`, `scanned_at`, or a full-scan time. The API response adds
`archive_digest_seeded`. Source nochange still requires a subsequent source
aggregate comparison with that seed. SQL admits SHA observations only for a
chunk with this original archive qualification proof; codec substitution into
an existing MD5/SHA receipt is refused.

The sealed baseline receipt shape is N part ACKs plus exactly one final ACK.
The final ACK has zero accepted/rejected rows and is bound to the single
`update_batch` for the same run/table/generations/physical name and payload.
It is not an extra data part. Initialization rejects missing/extra receipts or
a final record that is not the actual sealed batch.

Complete hydration for ALL parts of a table before its first incremental
commit: hydration is intentionally refused after the current table leaves the
original B/physical. Missing/corrupt retained artifacts are hard failures, not
permission to refresh. This amortizes initialization and makes it resumable;
it does not eliminate the eventual per-key membership storage.

## Partial observations and scan proof

`observe_partial` accepts `expected_source_generation=H` and:

```json
{
  "observation": {
    "scan_id": "scan-1",
    "schema_fingerprint": "<pinned source schema SHA>",
    "chunk_topology_revision": 1,
    "checked_at_ms": 0,
    "chunk": {
      "chunk_id": "0",
      "lo": null,
      "hi": [{"kind": "int", "value": 2}],
      "membership_revision": 1,
      "arrow_sha256": "<confirmed part or block SHA>",
      "row_count": 2,
      "digest": [2, "0000000000000001", "0000000000000002"],
      "digest_codec": "mysql-md5-xor/v1",
      "projection_sha256": "<baseline ordered-projection SHA>"
    }
  }
}
```

SQL is `c5_observe_partial(shard,owner,fence,source,schema,table,H,observation)`.
The values shown are illustrative, not precomputed data digests.
The worker must attach the actual `scan_id`; producer progress/work estimates
and `full_digest_scan_completed` are not authoritative. Worker changed-range
requests additionally carry the exact `range_scope`, actual `loader_receipt`,
and `observation.chunks` ordered prefix ending in `observation.chunk`. Native
state validates the range/lease/receipt shape, the SQL receipt guard matches
the actual block/run/apply receipt, and each prefix chunk is confirmed in the
same transaction. It does not silently confirm only the last chunk.

Initial digest registration requires either a successful original-part
hydration proof with unchanged membership/Arrow SHA, or a current sealed block
receipt and matching apply receipt. The producer MUST compare its newly read
Arrow with the retained part, or apply a changed block through the Loader,
before presenting this observation. Hydration alone does not prove the current
source is unchanged. Once recorded, a nochange observation must match the
stored digest, codec and projection exactly.

`digest_receipt` stores the digest with bounds, row count, membership revision,
Arrow SHA, codec and projection. `last_digest_scan_id` tracks actual digest
confirmation independently of the old commit-only `last_scan_id`.
`c5_digest_receipt_current(c5_chunk)` and `c5_digest_scan_prefix(...)` are
owner-only helpers available to 0067. `begin_scan` and export recompute the
contiguous confirmed prefix; a crash between commit and observe cannot skip a
block. Partial replay is idempotent. Only the last confirmed chunk completes
the scan and persists table/table-quality full-scan time. Membership, bounds,
Arrow, row-count or retirement changes invalidate the digest and full-scan
proof in the same transaction via `c5_invalidate_multipart_digest`.

Export adds chunk `row_count`, `digest_codec`, `projection_sha256`,
`last_digest_scan_id`, and active `scan_id`, `expected_chunks`, `scan_layers`,
`scan_observed_at_ms`. `begin_scan` accepts the four-value boolean layer vector
and observed-at milliseconds; `c5_scan_context` persists their first values
and refuses changed values on resume. With no active scan all four context
fields are JSON null and `next_chunk_index` is zero. An invalidated digest
exports as null, never paired with a newer revision.

The legacy `observe` operation for multipart tables routes its `chunks` through
the same receipt checks; B need not equal H. Explicit null scan ID AND null
expected chunks are allowed only without a running scan and cannot declare a
full-scan completion. This supports recent-window observations without
manufacturing a scan, source generation, table generation or publication.

The multipart range guard locks global H and actual table B/freshness, pins
the physical parent, scope and Loader-key hashes in `c5_table.multipart_parent`
with the current transaction ID, and refuses any remaining pending part.
After seal it checks that exact same-transaction parent and membership, the
real newly sealed table and H+1. Commit checks the pinned Loader keys before
delegating the existing receipted commit. Legacy non-multipart guards remain
unchanged. 0066 can wrap these functions rather than duplicate B/H handling.

## Verification

- `cargo test --locked -p oo-pipeline --lib native_c5_state::initialization_limits`
- `python3 crates/oo-pipeline/tests/support/run_c5_multipart_initialize_fixture.py`
  creates and removes only its own randomly named, network-isolated PG17.
- `OO_C5_INITIALIZER_OWNED=1 OO_C5_INITIALIZER_PG_PORT=<new owned PG17 port>
  cargo test --locked -p oo-pipeline --test native_c5_multipart_initialize_pg17
  -- --ignored --nocapture` uses a NEW disposable cluster only. It creates the
  fixed database `c5_initializer_owned` and installs real migrations, the real
  `jobs.loader_hold`, and synthetic sealed baseline receipts. Native state API
  calls then verify retained Arrow and hydrate through a non-superuser runtime.

These are local target/decoder fixtures. They are not a MySQL capture, overlay,
publication, live AgentDB, full-database load, or sustained production test.

The legacy sealed manifest does not contain key-column collations or per-row
part membership. Text boundary bytes and their source order can be preserved,
but PostgreSQL collation cannot establish MySQL membership. The deferred state
does NOT assert such equivalence. Numeric/binary predicates or verified original
part keys require explicit validation in 0066. Text chunks without trusted
source-order evidence must fail closed; no ctid/insertion-order heuristic.

## Replay and ownership

Initialization is transactional and if-absent. A replay checks the baseline
identity, fingerprint, topology and keys without replacing state, advancing
generations, resetting scans or rewriting membership. An existing single/
collapsed topology cannot be silently adopted. A later legitimate incremental
table generation is not a reason to reinitialize from a new full run.

All writes are target metadata only, behind the existing lease/fence binding.
No source DB writes, no live/refresh AgentDB testing, no migration registration,
target storage, worker, producer, publication or Gateway edits in this work.
