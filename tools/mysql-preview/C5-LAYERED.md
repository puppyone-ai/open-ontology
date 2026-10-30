# Layered MySQL C5 producer (W1-F2)

`mysql-preview c5-layered --config /absolute/operator.json` reads one existing
`oo.jobs.native-producer/v1` JSON document from stdin and prints one range request
or existing `oo.jobs.native-c5-nochange/v1` document. Errors use a closed stderr
message. It does not invoke a Loader, publish tables, or mutate customer sources.
`native-producer.py` and its default full behavior are unchanged.

## Installed binding

Add these fields to the existing job-worker native schedule manifest:

```json
{
  "c5_mode": "layered",
  "c5_layered": {
    "policy": {
      "period_ms": [60000, 120000, 3600000, 604800000],
      "duty_per_mille": 250,
      "threads_running_max": 8,
      "execution_window_utc": [0, 1440]
    },
    "window_ms": 300000,
    "overlap_ms": 120000,
    "recent_column": "changed_at",
    "server_timeout_ms": 5000,
    "client_timeout_ms": 10000,
    "execution_timezone": "UTC"
  }
}
```

Only C5 accepts these fields; missing c5_mode means full and serializes exactly
as before. Full cannot carry layered config. The existing binding poll interval
must be no greater than each layer period. All validation runs before registration.
Only explicit UTC is currently supported; overnight windows are supported, the end
is exclusive, and [0,1440] is all day. The recent column must lead an actual source
index. Null recent_column disables L2 without pretending there is a trusted cursor.

The worker pins c5_mode, c5_layered, c5_due and c5_observed_at_ms in producer_input.
The producer never chooses installed settings from its own output. `jw_native`
contains due/completed scheduling timestamps only; completion is in the existing
successful lease transaction. Source load is checked by the producer, not invented
by the worker's clock eligibility calculation.

## Operator file and authoritative state adapter

```json
{
  "source_id": "sample",
  "schema": "sample",
  "table": "items",
  "max_rows": 1000,
  "max_bytes": 1048576,
  "secret_ref": "secret://sample/reader",
  "connection": {
    "host": "fixture-host",
    "port": 3306,
    "user": "reader",
    "secret_file": "/absolute/reader",
    "tls": "required",
    "ca_file": "/absolute/ca.pem"
  },
  "target": {"database": "agent"},
  "state_dir": "/absolute/owned-cache",
  "authority_file": "/absolute/f1-export.json"
}
```

TLS uses the existing connector. Explicit disabled TLS additionally requires
plaintext_authorized=true and source have_ssl=DISABLED, as in the existing full
path. Password files use the existing 0600 reader. Each operation reuses a single
connection, including Scanner -> chunk::read_chunk transfer. Source grants are
validated by the existing read-only whitelist. Limits are bounded by the existing
typed chunk reader (100,000 rows and 16 MiB maximum). Statement and client deadlines,
Threads_running pause, UTC execution window and duty sleeps are applied.

**The authority_file is an adapter boundary, not an authoritative JSON database.**
F1's actual table/read/observation API was not part of the fixed request contract.
Before real operation, an internal F1 adapter must export a receipt-committed Agent
DB snapshot atomically to this file. Producer local cache.json is never read as the
source of truth. Each export has this shape:

```json
{
  "source_id": "sample",
  "schema": "sample",
  "table": "items",
  "source_generation": 3,
  "chunk_topology_revision": 1,
  "next_chunk_index": 0,
  "schema_fingerprint": "64-lowercase-hex-from-producer",
  "input_digest": "last-receipted-range-input-digest",
  "chunks": [
    {
      "chunk_id": "0",
      "lo": null,
      "hi": [{"kind":"int","value":1000}],
      "membership_revision": 1,
      "digest": [1000,"0123456789abcdef","fedcba9876543210"],
      "arrow_sha256": "last-complete-block-arrow-digest",
      "scanned_at_ms": 1792368000000
    }
  ]
}
```

Initialize generation from the actual acknowledged baseline, never from the cache.
For a new empty export, chunks=[], source_generation=0 and topology_revision=1 are
used by mock tests. Real initial full-load membership must be exported by F1. The
producer checks generation against the worker's acknowledged generation and fails
closed on schema drift or broken chunk adjacency. Chunk IDs are contiguous decimal
strings. Boundary int values are JSON integers; text/bytes values are lowercase hex,
not UTF-8 string comparisons. A finite tail range is appended using next_boundary;
existing boundaries are immutable. No client or PostgreSQL string ordering is used.

## One batch and retry

L1 reads positive hints without suppressing scans on NULL/unchanged timestamps.
L2 probes the indexed overlapping time window within each existing source range;
only ranges intersecting that window are candidates. L3 compares completed source
count/128-bit digests; differing ranges are reread in full. L4 rereads all ranges,
including equal digests, and compares deterministic typed Arrow digests. Complete
empty blocks are valid requests. One changed range is emitted per occurrence,
with parent_generation+1; unchanged typed blocks do not allocate a data generation.
A changed request reports no completed whole-scan layers. The final nochange reports
the due layers complete and carries c5_observation with scan metadata.

A per-binding/pin directory stores input, config hash, Arrow and immutable request.
The same pin returns identical request bytes; different input/config on the same pin
is refused. c5plus JSON writes retain pending-file/fsync fail-closed behavior. A crash
after a Loader receipt resumes from F1's exported membership/digests, not local cache.
A detected schema drift returns the closed SCHEMA_DRIFT stderr code and writes a
binding-local stop marker; later occurrences refuse before connecting or replaying
cached data. Clear that marker only after operator re-admission and F1 schema-state
repair. It is a fail-closed local safety latch, not authoritative data progress.
The mock test deletes cache.json between receipt and continuation and verifies the
completed membership revision is not reapplied. The receipt-backed next_chunk_index
resumes at the next pending range; final nochange resets it to zero for a new scan.
Absent cursor means zero for compatibility; invalid cursors fail closed.

## Coordinator's real F1 integration checklist

1. Install F1's append-only Agent DB migration; install the updated jw_native schema.
2. Connect the authoritative export above to F1's actual fenced scan/membership read
   API, including existing baseline membership, topology and prior revisions.
3. Pass every emitted range unchanged through F1 seal-native/admission binding and
   its real Loader (Arrow hash, boundaries, scope and prior revision must be sealed).
   The producer deliberately does not claim an admission receipt on its own.
4. Confirm the existing worker registration/receipt verifier accepts the new F1 range
   result and its control mirror; retain lease/source-bound/fence verification.
5. After each real receipt, export the committed chunk, Arrow digest, membership
   revision, next_chunk_index and source_generation. Never promote a pending producer cache record.
6. On final nochange, use F1's observation-only API to persist c5_observation and
   last_full_digest_scan_completed_at / recent_window_watermark without a new data
   generation or fabricated Loader data receipt. That observation writer is a F1
   integration dependency, not implemented by the JSON export reader.
7. Rerun fixture cases with real tombstones/receipt rollback/key reuse, then publish
   with the existing mechanism and query freshness with a read-only Gateway login.
   Verify source time remains unmeasured and the existing C5 readiness checks pass.

## Limits and upgrade conditions

- Mock Loader validation is not sealed admission, real receipt atomicity, publication,
  or whole job-worker-to-Loader E2E proof. Do not deploy until the checklist passes.
- No automatic chunk split/merge, adaptive retry on a now-oversized fixed range, or
  arbitrary IANA timezone support. Such a range fails its budget, never partially
  applies. Topology changes require the F1 fenced topology protocol.
- L2 materializes matching keys within a range via existing Scanner::rows. Very wide
  windows need a bounded/streamed near-window API before large-table calibration.
- New tail planning is one finite range per occurrence. A fully empty first table
  without an existing baseline cannot invent a finite boundary; initialize it using
  the existing full baseline rather than treating an empty initial nochange as a
  published generation.
- No C1 consistency upgrade, source timestamp, source position, or Ontology change
  is inferred from a scan. This remains C5/B/reconcile with DELETES_LAGGED.
