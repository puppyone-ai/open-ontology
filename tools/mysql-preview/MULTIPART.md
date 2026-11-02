# C5 multipart: bounded part streaming

This entry imports a chunk capture into one private table generation. It does not
invent a binlog position. The method is C5, the table grade is B, and the position
is the existing `snapshot-zero/v1` sentinel `0`.

## Capture and seal

Use the existing `oo.mysql-full/v1` capture configuration with `chunking: true`.
For the six-row M1 fixture, `budgets.max_rows: 2` produces exactly three parts.
Keep the normal byte/statement/deadline budgets; no limit is raised.

```
mysql-preview capture-full --config /private/capture.json --out /private/capture
python3 tools/mysql-preview/prepare-native.py --capture /private/capture \
  --out /private/inputs --secret-ref secret://fixture/reader \
  --source-audit-policy grant_enforced
oo-source-admission seal-native --report /private/inputs/items/report.json \
  --config /private/inputs/items/config.json --out /private/sealed
```

The grant-enforced switch is an operator choice, not inferred from capture.
For `mysql.general_log`, omit it and use the existing independently authorized
observer configuration. Both paths verify each successful chunk connection's
source audit envelope.

`capturer.json`, the admission receipt and the stamped registry contain
`multipart_scope` (`oo.mysql.c5-multipart/v1`). It binds source, table, schema
preimage SHA-256, capture configuration SHA-256, occurrence/run identity,
source generation, the canonical JSON chunk-manifest SHA-256, the exact ordered
chunk entries and every part's zero-based index, total count, chunk ID, rows and
Arrow SHA-256. Normal admission configuration/type/schema/audit digests remain
bound by the existing seal. Files are canonical `<index>.arrow`; missing files,
extra Arrow files (including numeric aliases), duplicates, reordering, manifest
changes and altered bytes refuse sealing. `table-0.arrow` is the legacy combined
stream, not an extra part.

## Loader protocol and ACK

Install with the existing `oo-pipeline install-native-update` command. This adds
checksum-checked migration `0053_native_c5_multipart.sql`, version
`native-c5-multipart/v1`. Published older migration files are unchanged.

Invoke `oo-pipeline import-native-update` with this envelope on stdin:

```json
{
  "protocol": "oo.pipeline.native-c5-multipart/v1",
  "native_request": {"protocol": "oo.pipeline.native-update/v1"},
  "multipart_scope": {},
  "action": "part",
  "part_index": 0
}
```

The nested request is the complete existing source-bound native request, not just
the abbreviated example above. The scope must equal the installed sealed registry.
Set `full_snapshot: true`, `consistency_grade: "B"`, `mirror: false` and retain the
same source generation, batch/run identity, target shard and target owner through
retries. Supply a real `jobs.claim` task ID and current lease generation. This M1
entry requires jobs and target in the same PostgreSQL database; it calls the
existing `jobs.loader_hold`, never inserts or fabricates a claim. The operator
must install the usual immutable `OO_NATIVE_ADMISSION_BIN`,
`OO_NATIVE_BINDING_REGISTRY` and `OO_NATIVE_BINDING_REGISTRY_SHA256` values.
There is no caller-configurable trust or fence bypass.

Send parts in order. Each successful COPY and its ACK commit together using
`_oo.full_import_write_batch` / `_oo.apply_receipts`. The part batch identity is
`<capture occurrence>.part.<index>` and the payload digest is that part's Arrow
SHA-256. Replay verifies the artifacts and returns `part_acknowledged` with
`replay: true`, without another COPY. The same `_oo.full_import_run` and mapped
shadow are recovered after a killed Loader. Target fencing epochs may advance
when a new connection adopts the shard; this does not create a new source/table
generation or a new shadow.

All parts remain private. A semantic part failure retires the entire candidate;
it cannot be resumed or published. A process kill rolls back its uncommitted
part and leaves the private candidate resumable. Never edit ACKs, fences or
watermarks to recover.

## Reconcile, seal and publish

For a frozen bounded source, run the existing independent cell oracle through:

```
source-onboarding full-audit --config /private/audit-config.json \
  --catalog /private/audit-catalog.json --targets /private/audit-targets.json \
  --out /private/audit-evidence
```

This calls `audit.rs::compare_full`, opening source and target independently of
the capture and Loader. Catalog/targets use its existing table/physical-column
shape. Require zero typed differences. Then send the same multipart envelope
with `action: "finalize"`, omitting `part_index`, and place the independently
read typed cell rows in `native_request.truth`. The Loader requires complete
ACK coverage and exact target-cell equality before the existing full-import
seal can produce `awaiting_publish`. Its final zero-row bookkeeping receipt is
not another Arrow part. Publish through the existing operator
`_oo.publish_native_per_table_collection` API. This is native data-generation
publication, not Ontology/business review or Agent-role authorization.

The anonymous, executable fixture harness is
`testing/mysql-full-c5/g1-mpart/m1_fixture.py`. Its `setup`, `both`, `kill` and
`refusals` commands exercise both audits, actual SIGKILL recovery, independent
comparison, native publication, seal refusals and candidate retirement.
See `M1-REPORT.md` beside it for exact resource/build/run commands and evidence.

## Limits and upgrade conditions

- M1's legacy combined-Arrow witness remains readable, but the producer now uses
  `prepare-native.py --multipart`: a bounded part-0 anchor plus the complete sealed
  manifest. The sealer hashes all parts one at a time; the Loader type-checks,
  copies, reads back and ACKs each bounded part in one transaction. It never reads
  the combined table Arrow. The 32 MiB per-file and 100,000-row per-part caps remain.
- The normal small-table request shape stays unchanged. A chunked request adds
  `multipart_scope` on the same native-update/v1 request. The existing worker invokes
  Loader once; Loader streams the parts under that real claim and returns the normal
  authoritative receipt/mirror result. Full-batch routing and ledger metrics are
  wired through the existing callback; no second scheduler/Loader is used.
- `full-audit --request NATIVE_REQUEST` selects the bounded chunk-membership
  oracle. It re-reads source ranges and target typed cells independently. A changed
  source chunk is unresolved, never zero differences. The ordinary no-request
  invocation keeps the original bounded-table oracle.
- Failed conversion/oversized source rows remain fail-closed candidates. Their masked
  observations are persisted in sync.rejects using the existing leased/fenced state
  entry; they are not admitted data or successful generation ACKs.
- Binary trust remains measured and pinned. The M1 pins identify the retained
  host fixture executables. W1-F must build and review its Linux artifacts and
  register their exact digests/resources by the existing trust process; no
  manifest or environment value can self-approve another binary.
