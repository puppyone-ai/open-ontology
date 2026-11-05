# Lane F document sources (S5-F-02)

Partial delivery: not full T0-T2/T8 acceptance. All sources remain opt-in.

## Product surface

- MongoDB: bounded `_id` snapshots pinned to a majority-committed cluster time,
  explicit scalar projections plus a BSON Extended JSON document. The source
  change-stream implementation retains opaque resume tokens, source epoch and
  same-timestamp ordering, and requires exact post-images. Product C1 remains
  rejected: `PgSync` needs the comparator extension in `coordinator.patch`.
- MongoDB C5 reuses persisted key ranges, validates their shape and continuity,
  and rejects over-budget growth rather than truncating it. Growth splitting
  of a persisted range is not implemented.
- OpenSearch/Elasticsearch: verified HTTPS, explicit concrete index scope,
  PIT/search_after snapshots, PIT cleanup and partial-shard/timeout rejection.
  No log CDC. C5 uses fixed ranges of an existing single-valued keyword field.
- All three CLI snapshot/C5 paths store `_document` as PostgreSQL jsonb through
  an explicit target opt-in. Ordinary text remains text. Scalar projections
  support int64/float64/bool/text; missing and null project to SQL NULL, while
  the full document preserves their distinction.
- Digests scan bounded full documents on the client. Idle C5 has zero chunk
  payload and target rewriting, **not zero source network traffic**.

Build before starting any fixtures:

```sh
cargo build --manifest-path tools/oo-load/Cargo.toml --locked --features document-sources
cargo test -p oo-connectors --locked --features mongodb,opensearch --test document_live --no-run
```

Set `OO_MONGO_COLLECTION_CONFIG` to a JSON file mapping collection names to
scalar declarations, e.g. `{"events":[{"name":"value","kind":"int64"}]}`.
Authentication database is currently `admin`. Set `OO_SEARCH_INDEX_CONFIG` to:

```json
{"events":{"range_field":"key","boundaries":["0050","0100","0150"],"columns":[{"name":"value","kind":"int64"}]}}
```

Scope must match exactly. Credentials remain stack `secret_ref`s, never argv.

## Reproduction and evidence

```sh
python3 testing/engines/document/run.py --engine mongodb --run-id UNIQUE_RUN_ID
python3 testing/engines/document/run.py --engine opensearch --run-id UNIQUE_RUN_ID
python3 testing/engines/document/run.py --engine elasticsearch --run-id UNIQUE_RUN_ID
tools/oo-test/oo-test matrix --engine mongodb --versions 8.0.34 --suites T0-T2
python3 -m unittest discover -s tools/oo-test
```

The runner creates at most two owned `oo.lane=f` containers (source + Agent DB),
uses runtime-generated credentials and separate fixture-admin/read-only users,
and removes only resources it owns. Search security and certificate validation
are enabled. No privileged containers, sysctl or binfmt changes are used.
Only linux/arm64 has been executed. Pins retain lane H's format.

`product.py` compares typed scalar values and exact JSON documents against a
separate admin export. A writer starts after the first committed target chunk,
while the source snapshot remains pinned. Three C5 cycles include update,
insert, delete, idle, and a subsequent update. The idle cycle asserts zero
chunk payload, target write bytes and new generations. This is a small
200-document fixture, not the full 2M-row/300-column conformance dataset.

Raw outputs and explicit `verdicts.py` rows live under ignored `evidence/`.
The matrix command still returns nonzero: product `oo doctor` (T0), measured
source connection peaks (T8), transactional MongoDB C1 and actual eviction of
a valid resume token are **not accepted**. Source-only checks must not promote
those rows. The source retained-floor negative test is not token eviction.

## Shared boundary

`coordinator.patch` now contains only the exact, unapplied `PgSync` comparator
extension. `engine_sync_pg.rs` remains outside this card's explicit shared-file
grant. The jsonb path was implemented without modifying the shared Arrow
parser. The remaining C1 work requires transaction/replay tests plus snapshot
alignment and durable-token integration; applying the patch alone is not C1
acceptance. The Python comparator hook is implemented in
`tools/oo-test/document_compare.py`.

The standalone `tools/oo-load/Cargo.lock` adds the optional MongoDB graph;
root Cargo.lock is unchanged. D18's existing tiny-keccak exception permits
cargo deny; no license or security policy was weakened.
