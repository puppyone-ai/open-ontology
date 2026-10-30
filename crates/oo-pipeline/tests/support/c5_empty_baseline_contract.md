# Empty-only C5 confirmation (0071)

Implemented/frozen SQL contract: `native-c5-empty-baseline/v1`, after 0070.
0069 stays byte-for-byte frozen. No source writes, full refresh, invented keys,
range state, data generations, apply receipts or ACKs are introduced.

## Worker

Use an explicit `Empty` initializer, never an automatic fallback after a keyed
initializer fails. `key_columns` must be `[]`, including for unsupported-key
tables; this means no key-based operation is allowed, not that the source lacks
a declared key. Pin the real schema fingerprint, including the source key facts.

Call `initialize_empty_if_absent` using the normal state envelope, lease,
`schema_fingerprint`, `chunk_topology_revision: 1`, `key_columns: []`,
`expected_source_generation: H`. Do not supply a baseline, count or manifest.
Target verifies the genuine sealed SINGLE baseline at B <= H, with zero rows,
zero tombstones/rejects, its original raw input digest and exact apply receipt.
No Arrow hydration or `begin_scan` follows. Export through the existing `export`
operation; the worker must explicitly validate the empty authority shape.

Authority has `protocol: "oo.pipeline.native-c5-empty-authority/v1"`,
`empty_only: true`, global `source_generation`, real `table_source_generation`,
`schema_fingerprint`, `input_digest`, `baseline_receipt`, `selected_columns`,
`columns`, `projection_sha256`, `type_digest`, `type_policy_version`, and:

```json
{
  "chunks": [],
  "key_columns": [],
  "scan_id": null,
  "expected_chunks": null,
  "scan_layers": null,
  "scan_observed_at_ms": null,
  "next_chunk_index": 0,
  "empty_observation": {
    "id": "target-issued UUID",
    "lease_id": "current task UUID",
    "lease_generation": 1,
    "source_generation": 7,
    "issued_at_ms": 0
  },
  "last_empty_confirmed_at_ms": null
}
```

The initializer reuses a context for the same lease/generation/H. A different
live lease cannot replace it. A new occurrence receives a new observation ID;
previous observation data cannot certify it. The original B is never advanced.

## Kant producer

Use normal approved producer execution, read-only authorization, source budgets
and the read-only consistent-snapshot path. Validate the pinned schema before
and after the unfiltered probe, including declared keys even when their types
cannot be used by the range codec. Use the actual full table, not a projection,
partition, sampled rows, count estimate, filter, or retained source_counts.

The exact query is `SELECT 1 FROM \`schema\`.\`table\` LIMIT 1` (double embedded
backticks in each identifier). Zero returned rows proves emptiness at this
snapshot. ANY returned row is `C5_EMPTY_BASELINE_NONEMPTY`: hard refusal, never
range/full capture, ignored rows, or fabricated nochange. Timeout, schema drift,
incomplete snapshot and authorization failure also refuse.

Return the existing `oo.jobs.native-c5-nochange/v1` envelope: same source/H and
acknowledged H, `parent_digest` equal to authority input_digest, normal binding,
pin, source_digest/snapshot, `empty_diff: true`, and data_gen/apply/ack all false.
Its `c5_observation` must be exactly:

```json
{
  "protocol": "oo.c5.empty-observation/v1",
  "source_id": "source",
  "schema": "schema",
  "table": "table",
  "source_generation": 7,
  "table_source_generation": 3,
  "schema_fingerprint": "pinned SHA256",
  "baseline_receipt": {},
  "observation_id": "exported empty_observation.id",
  "lease_id": "exported lease_id",
  "lease_generation": 1,
  "started_at_ms": 0,
  "checked_at_ms": 1,
  "source_query": "SELECT 1 FROM `schema`.`table` LIMIT 1",
  "rows_returned": 0,
  "row_limit": 1,
  "snapshot_complete": true,
  "read_only": true,
  "schema_checked_before": true,
  "schema_checked_after": true
}
```

Copy the complete trusted `baseline_receipt` from authority. Start the source
probe after the context is issued; report actual start/completion times. There
are no fake chunks/digests or `full_digest_scan_completed` claim. Scheduler layer
completion must remain an explicit worker policy, not inferred from archive data.

## Target and freshness

Send this nochange through the existing state `observe` operation. The empty
protocol selects the separate 0071 confirmation function. The pipeline holds
the real jobs lease and source fence; SQL rechecks the live lease, current H,
real table B/physical baseline, receipt/schema/config/type/projection identity,
observation ID, exact query, zero result, snapshot flags and observation times.

Only then is `last_empty_confirmed_at_ms` persisted. Exact replay is idempotent
and does NOT refresh that timestamp. Existing table applied_at, source watermark,
table generation, C5 digest-scan timestamp, apply receipts and ACK state remain
unchanged. Freshness consumers must use this explicit evidence, not label it a
digest scan or manufacture a new data generation.

All seven customer exceptions remain in the coverage denominator. This contract
is zero-only: their first source row remains a visible hard refusal until a
separate supported incremental identity protocol exists. Main owns worker
integration, installer registration and product nochange E2E acceptance.

## Target acceptance

0071 SHA256:
`49a89ffa104c574ef10212d57d3488cf438ac1147bd3515bdbb5029d5eafd430`.

Sixteen state unit tests pass, including float archive seeding and empty-only
request refusals. The Native State -> non-superuser owned PG17 test passes for
three genuinely zero-count sealed fixture baselines at B=1/2/4 under H=7:
nullable/keyless, DATE and NUMERIC candidate columns. It verifies confirmation,
exact replay without timestamp refresh, wrong query/result/schema/config/nonce,
incomplete snapshot, expired lease, live-context replacement refusal, H=8 with
unchanged B=1, and no data watermark/freshness/receipt or range-state mutation.
The first test attempt exposed an OID JSON string; the unfrozen migration was
corrected to emit numeric OIDs and the complete fixture passed on a new cluster.

```sh
cargo test --offline --locked -p oo-pipeline --lib native_c5_state::initialization_limits
OO_C5_EMPTY_OWNED=1 OO_C5_EMPTY_PG_PORT=<fresh-owned-PG17-port> \
  cargo test --offline --locked -p oo-pipeline \
  --test native_c5_empty_baseline_pg17 -- --ignored --nocapture
```

The ignored test requires a new disposable cluster, installs 0000..0071 and
uses real lease/fence functions. Sealed baseline receipts and source observation
documents are synthetic test inputs. This proves target checks, NOT an actual
MySQL emptiness probe, producer admission or customer E2E. Main/Kant own those
remaining integration gates. Logs: `/tmp/c5-0071-unit.log` and
`/tmp/c5-0071-pg-2.log`; the earlier failure is retained separately.
