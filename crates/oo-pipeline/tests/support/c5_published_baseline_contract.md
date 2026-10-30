# 0069: retained single and multipart baselines

Owner: initializer/state agent. Main owns installer registration. Maxwell owns
the worker adapter. No source/EIP access or full-refresh fallback is introduced.
0065, 0066, 0067 and 0068 are unchanged by this work.

## Worker / installer contract

- Version: `native-c5-published-baseline/v1`, file
  `0069_native_c5_published_baseline.sql`, after exact 0068.
- New `Initializer::Published` dispatches `initialize_published_if_absent`.
- The request uses the existing source/schema/table/target/lease fields,
  `schema_fingerprint`, `chunk_topology_revision: 1`, ordered `key_columns`,
  and `expected_source_generation: H`. No supplied bounds or manifest.
- `key_columns` is the operator-pinned SOURCE key order. Do not derive it from
  Arrow field order, PostgreSQL attribute order or the target unique index.
- Continue using `hydrate_multipart` for both kinds, with the same private
  `baseline_parts` map. A single baseline uses key `"0"` pointing to its original
  raw table Arrow. A multipart baseline uses the original numbered part files.
- Hydrate pending chunks before source reads or any incremental commit. Export,
  begin_scan, observe_partial and observe retain the 0065 interface.

SQL initializer:

```text
c5_initialize_published_if_absent(shard, owner, fence, source, schema, table,
                                  fingerprint, topology, keys,
                                  expected_source_generation) -> jsonb
```

The initializer selects the actual sealed run for the current physical table.
It accepts `awaiting_publish` or `published`, not an open/retired run. B is the
table's real generation; H is the current source-wide watermark; B <= H is
normal. Neither is rewritten. Existing mismatched/folded C5 state is refused.

## Evidence and topology

`_oo.c5_multipart_baseline` gains `baseline_kind`, either `single` or `multipart`.
The old table name is retained so 0065 guards, 0066 overlay and 0067 publication
can reuse their existing metadata relationship. Export contains that kind at
the top level and inside `baseline`, alongside the real selected columns,
schema/config/type digests and `type_policy_version: "type-policy/v1"`.
Normalization flags are NOT inferred from a hash; use the pinned retained
producer configuration for those flags.

Single is determined exclusively by SQL NULL `full_import_run.multipart_scope`.
The original run is never changed and no multipart manifest is synthesized.
One null/null C5 topology chunk references `r.input_digest`. A single receipt
helper verifies exactly one apply receipt, quality batch and update_batch,
same run/source/table/physical/generations/payload/config/schema/type, accepted
rows equal the sealed table count, zero rejects and no tombstones.

Historical single loads need not have a unique target index. Initialization
checks mapped, non-null supported key columns; hydration independently verifies
unique typed identities across the entire SHA-bound Arrow before clearing
pending. Empty single tables WITH a pinned supported non-null stable key are
supported with a genuine empty Arrow/receipt. This is not keyless-table support.
The existing 32 MiB artifact and 100,000-row per-call bounds remain in force.

Multipart keeps the real sealed manifest and N-part-plus-final-ACK proof.
The target unique index must cover the exact key SET, but its order is ignored.
The pinned source order drives typed bounds and member tuples. Both Rust and SQL
compare the last retained row's tuple to the actual captured manifest `hi`.
This includes the last part, whose C5 outer high edge is null but whose captured
high tuple remains available. Mismatch fails atomically; no collation guessing
or reordering is permitted.

`c5_multipart_hydration_manifest` now returns:

- `baseline_protocol: "oo.c5.published-baseline/v1"`
- real `baseline_kind`, `baseline`, `columns`, chunk SHA/count
- multipart: the unchanged `multipart_scope`, converted real `capture_hi`
- single: JSON-null `multipart_scope`/`capture_hi`, a `single_receipt` bound to
  the actual sealed run and raw input digest

The state API validates the exact 0069 migration checksum before consuming this
new authority shape. The existing hydrate and archive-SHA seed operations work
for either kind. They do not create data generations or apply receipts, and a
seed is still not a source scan/nochange assertion.

## Acceptance

The new owned-PG test uses two tables sharing one source: a raw single baseline
at B=3 with global H=7, and a three-part composite-key table whose physical
column/index order is `[b,a]` but pinned source key order is `[a,b]`.
The required positives are initialization, exact retained membership, SHA seed,
export B/H preservation and idempotent replay. Negative cases cover a reversed
source-key pin/boundary mismatch, receipt/count/SHA corruption and no invented
multipart scope for single. This is a local target/Arrow fixture, not customer
or source-to-publication acceptance.

The same fixture also covers an empty single at B=1, and begin_scan/nochange
completion for all three tables without advancing H or their real generations.

```sh
cargo test --offline --locked -p oo-pipeline --lib native_c5_state::initialization_limits
OO_C5_PUBLISHED_OWNED=1 OO_C5_PUBLISHED_PG_PORT=<new-owned-PG17-port> \
  cargo test --offline --locked -p oo-pipeline \
  --test native_c5_published_baseline_pg17 -- --ignored --nocapture
```

Run the ignored test only against a new disposable owned cluster. It installs
the actual 0000..0069 SQL chain, uses real lease/fence functions and a non-superuser
runtime, but constructs synthetic sealed receipts; it is not source admission
acceptance. Main must confirm the retained single inventory fits the declared
32 MiB / 100,000-row bounds before rollout.

## Empty-table coverage gate

The positive empty fixture declares `key_columns: ["id"]`; it does not prove
support for the seven reported customer empty-baseline exceptions. Keep all
seven in the coverage denominator until their retained schema/key evidence is
classified. No customer/source query is authorized by this contract.

Lack of a primary key is not necessarily lack of a usable stable key: a pinned
supported non-null unique source key may qualify. A genuinely keyless request
(`key_columns: []`) is rejected by both the Rust request validator and SQL.
Do not invent a key, omit the table, mark it scanned from archive evidence alone,
or silently fall back to full refresh. An empty archived table does not prove
that the current source is empty or that future rows have stable identities.
Keyless support needs a separately coordinated producer/worker/state protocol
and tests for empty-to-nonempty transitions; it is not implemented by 0069.
