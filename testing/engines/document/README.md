# Lane F document sources (S5-F-02)

This is a **partial delivery**, not T0-T2/T8 acceptance or a verified support
matrix entry. Source engines and their isolated live tests exist; the MongoDB
Agent DB boundary still needs coordinator changes.

## Product surface

- `oo-connectors/mongodb`: `MongoEngine` implements `SourceEngine` and
  `ChangeSource`. Replica-set ID, collection UUIDs and explicit projections
  bind the source epoch. Snapshots use one majority-committed `atClusterTime`,
  simple-collation `_id` bounds and bounded cursor pages. Mixed `_id` BSON types
  are rejected, not silently skipped by MongoDB's type-bracketed comparisons.
- Change streams request **required** post-images, never `updateLookup` images
  from a later time. Positions retain the opaque Extended JSON resume token,
  cluster timestamp and sequence. They are serialized for caller persistence;
  the library does not claim an atomic Agent DB commit. Expired history,
  changed epochs and DDL fail closed. Same-timestamp events retain stream order.
- `oo-connectors/opensearch`: explicit OpenSearch/Elasticsearch dialects,
  verified TLS, exact concrete index scope, PIT renewal/cleanup, `search_after`,
  partial-shard/timeout rejection and no log CDC. The shared family enum calls
  both products `Opensearch`; the probe always names the actual product.
- Scalar projections are explicit `int64`, `float64`, `bool` or `text`.
  Missing and null both project to SQL NULL; `_document` distinguishes them and
  preserves full BSON Extended JSON for MongoDB. Arrow marks that column with
  `oo.document=jsonb`.
- Search C5 ranges require a pre-existing, unnormalized, single-valued keyword
  field and caller-specified fixed boundaries. The reader creates no field or
  index. Null/missing/multivalued range keys are rejected inside the PIT.
  Digests scan bounded full documents on the client; this is not a source-side
  zero-payload digest optimization. Oversized ranges fail rather than truncate.

`oo-load --features document-sources` dispatches search snapshots and `--c5`
through the existing target. Set `OO_SEARCH_INDEX_CONFIG` to a JSON file:

```json
{
  "events": {
    "range_field": "key",
    "boundaries": ["0050", "0100", "0150"],
    "columns": [{"name": "value", "kind": "int64"}]
  }
}
```

The source scope must exactly match those indices; its database is the logical
schema name. Credentials still come exclusively from stack `secret_ref`s.
Until the shared patch is reviewed, search `_document` is stored as **text**.
MongoDB CLI dispatch refuses before connecting rather than silently substituting
text for jsonb or dropping the durable resume token.

## Reproduction

```sh
cargo test -p oo-connectors --locked --features mongodb,opensearch --lib
python3 testing/engines/document/run.py --engine mongodb --run-id f-doc-mongo
python3 testing/engines/document/run.py --engine opensearch --run-id f-doc-os
python3 testing/engines/document/run.py --engine elasticsearch --run-id f-doc-es
tools/oo-test/oo-test matrix --engine mongodb --versions 8.0.34 --suites T0-T2
```

Each direct runner uses one fresh `oo.lane=f` container, runtime-generated
credentials in an ignored private directory, a separate admin identity and
cleanup restricted to its own run labels. Search fixtures enable security and
verified HTTPS. No privileged containers, sysctl or binfmt changes are used.
The matrix command deliberately exits nonzero even after passing source checks:
source-only success must not be promoted into product acceptance.

Pins use lane H's `images.pins`/`images.lock.json` shape. The OpenSearch arm64
pin is a registry-verified, locally cached 3.9.0 build; it is intentionally not
the subsequently rebuilt mutable tag's arm64 digest. Only arm64 was executed.

## Remaining integration

`coordinator.patch` is an exact, unapplied proposal (passes `git apply --check`):
preserve the document annotation in decoding, map it to PostgreSQL jsonb, and
allow `PgSync` to use a caller-supplied epoch-bound comparator while retaining
its atomic position update. It does not by itself finish MongoDB CLI sync,
initial C1 alignment, or C5 persisted range reuse.

Required follow-ups:

- Review the shared patch and test it against snapshots, C5 and transactional
  C1 (including crash/replay); then replace the explicit MongoDB CLI refusal.
- Persist and reuse MongoDB C5 boundaries across processes, including growth
  splits. Fresh key sampling after insert/delete can change the plan.
- Run full Agent DB comparisons, larger/heterogeneous fixtures and measured
  source connection peaks/target write counts. Driver monitoring adds MongoDB
  connections beyond its application pool of one.
- Exercise actual oplog eviction of a valid resume token. The current test
  rejects a position below the measured retained floor, not forced eviction.
- Resolve the feature-only `cargo deny` CC0-1.0 rejection for transitive
  `tiny-keccak` through the coordinator's supply-chain process or a driver
  replacement. No license policy was relaxed and MongoDB is not default-enabled.

## Resume validation (2026-11-05)

The retained CLI and fixture changes were committed, then rebased onto
`9af453c7e` (`v0.2/product-loops`). The proposed patch was refreshed for the
new `numeric(20,0)` mapping without changing any shared target source file.
Raw validation output is in `evidence/f-s5-resume-20261105/`.

- Rebased workspace locked check passed.
- Document-enabled `oo-load` tests passed: 35 passed, 7 live tests ignored.
- Document-enabled connector library tests passed: 124 passed.
- Full connector tests reproduce the pre-existing macOS failure
  `parse_file_cli::parent_deadline_kills_the_worker_and_the_next_call_completes`
  (`group_signal=rejected`, expected `sent`). A serial run on archived base
  `9af453c7e` fails identically. With only that test excluded, 234 pass,
  25 are ignored, and one is filtered out. The earlier overlapping base run
  encountered executable-hash replacement; its output is retained separately.
- Both lockfiles passed `cargo audit`. The search-enabled tool passed
  `cargo deny`; the MongoDB-enabled workspace still fails the `tiny-keccak`
  CC0-1.0 gate. No supply-chain policy was weakened.
- Archived source-only results were inspected: `f-s5-mongo-08` (8.0.34),
  `f-s5-os-04` (3.9.0), and `f-s5-es-04` (9.2.0). All explicitly mark
  T0/T1/T2/T8 as not accepted and report successful container cleanup.
- No fresh database fixtures were started: after builds, available host
  space was below the repository's 20 GiB dispatch floor. This is not a
  fresh live-engine pass. No containers were created during this resume.

S5-F-02 remains incomplete. In particular, applying `coordinator.patch`
alone is not acceptance: MongoDB CLI baseline alignment, durable C5 range
reuse, actual history eviction, Agent DB comparisons and measured T8 resource
limits still require implementation and live verification.
