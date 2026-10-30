# Native C5 range v1

This additive protocol does not change native-update, native-partition or
`full_snapshot`. Implementation status must be checked against the delivery report;
this document alone does not enable layered loading.

## Closed request and sealed scope

`protocol` is `oo.pipeline.native-c5-range/v1`. The closed range scope has:
`source_engine` (`mysql`), `capture_method` (`c5_diff`), `source_id`, `schema`,
`table`, `schema_fingerprint`, `chunk_topology_revision`, `chunk_id`, `lo`, `hi`,
`prior_membership_revision`, `parent_generation`, `source_generation`,
`arrow_sha256`, `row_count`, `delete_detection` (`reconcile`), `position_codec`
(`snapshot-zero/v1`), `applied_pos` (`0`).

Additional fields reserved by this implementation:

- `members`: the complete new block's typed key identity set, including `[]`
  for an empty block. Each member is a nonempty key array. Bind this array in
  admission evidence together with every scope field, not just row count.
- `lo`: null for the first block; otherwise a nonempty typed key array.
- `hi`: null for an unbounded final block; otherwise a nonempty typed key array.
- `parent_generation`: nonnegative integer (zero is the initial parent).
- `prior_membership_revision`: nonnegative integer, zero for initial membership.
- `chunk_topology_revision`: positive integer; changed topology uses new chunk IDs
  and a full read before retiring parents.

A key component is exactly `{ "kind": "int"|"text"|"bytes", "value": ... }`.
Integer values are signed 64-bit JSON integers. Text and byte values are lowercase,
even-length hex (including empty strings); text is source bytes, not normalized
Unicode. Composite keys preserve component order. Duplicate member identities,
unknown fields, malformed digests and inconsistent key arity/kinds are rejected.
`schema_fingerprint` and `arrow_sha256` are lowercase SHA-256 hex.

The protocol is a scope, not arbitrary SQL: no WHERE/predicate field is accepted.
Loader admission must compare the entire scope with sealed source evidence;
caller-provided scope equality without admission is insufficient. The immutable
Arrow stream's complete SHA-256 and decoded row count must match, including empty
blocks. Each decoded row key must occur exactly once in `members`, and each member
must have a row. Membership identity comparison is byte equality only. No Rust or
PostgreSQL string ordering may be used to infer `(lo, hi]`; the admitted source
reader owns collation-aware boundary selection and declares block membership.

## Target transaction contract

Under the existing lease and generation fence, update membership, write DEC-07
missing-member tombstones, upsert rows, seal the real Loader receipt and advance
that block checkpoint in one target transaction. Only a sealed receipt advances
progress. Retry before commit uses identical bytes and scope; retry after commit
replays the receipt. Old generations cannot resurrect tombstoned keys. Missing or
unscanned blocks never imply deletion. Promote a scan only when all required
blocks have receipts; resume the first unreceipted block.

## Observations and F2 interface

Agent DB quality fields are nullable `c5_mode`,
`last_full_digest_scan_completed_at`, `recent_window_watermark`. They are
observations, not source commit positions or readiness proofs. Absent mode means
`full`. Preserve `c5_diff`, `reconcile`, `DELETES_LAGGED`, B chunk consistency,
`CROSS_TABLE_SKEW`, and source time `unmeasured`.

Nochange keeps `oo.jobs.native-c5-nochange/v1`, `data_gen=false`, `apply=false`,
`ack=false`; observational updates must not fabricate Loader data receipts.

## Implemented Loader envelope and state operations (W1-F1 continuation)

`import-native-update` now dispatches this protocol. Keep the twenty closed scope
fields above at the top level and append `native_request`: the existing admitted
native-update request, including Arrow path, frozen columns/keys/digests, snapshot,
operator registry/admission references, target connection and live jobs lease.
`native_request.full_snapshot` must be false, tombstones must be empty, consistency
must be B/per-table, and all overlapping identity/digest/position fields must match.
The Loader derives deletions from stored membership, not producer tombstones.

The producer's known transport/observation fields are also accepted:
`input_digest`, `arrow_path`, `consistency_grade`, `cross_table_consistent`, `target`,
`c5_completed_layers`, `c5_observation`, `binding`, `pin`. The first five must equal
`native_request` when supplied; the last four do not authorize data changes or
checkpoint promotion. Other envelope/native-request fields are rejected. The
capturer's `range_scope` and the registry binding's `range_scope` are exactly the
closed twenty-field scope, excluding `native_request` and transport fields.
Existing source evidence validation is still mandatory. A range-bound admission
cannot be used on the old native-update/full_snapshot route.

Migrations 0047–0052 install `_oo.c5_table`, `c5_scan`, `c5_chunk`, `c5_member` and
`c5_block_receipt`. Runtime has no direct DML on these tables. The existing Loader
calls `c5_range_guard` after binding the target fence and before creating a shadow;
it then uses existing copy-forward/COPY/upsert/tombstone/quality/seal helpers, and
calls `c5_range_commit` after the real run is sealed, before transaction commit.
The block receipt references and verifies that sealed run and its apply receipt.

Use `oo-pipeline native-c5-state` with JSON stdin:

```json
{
  "protocol":"oo.pipeline.native-c5-state/v1",
  "operation":"export",
  "source_id":"sample","schema":"app","table":"items",
  "target":{"host":"127.0.0.1","port":5432,"database":"agent",
    "runtime_principal":"reader","password_file":"/absolute/private-password","ssl":"disable"},
  "authority_file":"/absolute/owned-directory/authority.json"
}
```

Remote targets retain the existing verify-full/CA requirements. `export` reads a
single SQL snapshot via `_oo.c5_authority_export` and returns F2's authority shape.
Optional `authority_file` uses private create-new staging, file fsync, rename and
parent-directory fsync. No producer cache is read.

Write operations additionally require `lease_id`, positive `lease_generation`,
and target `shard`/`owner`; they lock the real jobs claim and use the existing
adopted target fence:

- `initialize_full`: `schema_fingerprint`, `chunk_topology_revision`, numeric
  string `chunk_id`, `key_columns`. Requires the current actual sealed full-read
  receipt. Reads all target key identities into one unbounded chunk. On a topology
  change it requires a newer full data generation, next topology revision and new
  chunk ID, and no running scan. Old chunks/receipts are retained, marked retired,
  and excluded from active exports. This is conservative full-read rebasing, not
  automatic split/merge. A baseline that exceeds one range's read budget needs a
  reviewed bounded topology initializer before layered operation.
- `begin_scan`: `scan_id`, `expected_chunks` (ordered array of chunk IDs). Reusing
  the same running manifest returns its first unreceipted block. Different active
  identities/manifests are refused. Missing expected chunks remain pending.
- `observe`: `nochange` containing the unchanged
  `oo.jobs.native-c5-nochange/v1` document. It requires data_gen/apply/ack=false and
  matching acknowledged/current generation. A complete observation must enumerate
  all active blocks and match committed Arrow hashes, revisions, bounds and counts.
  It atomically promotes 128-bit digest observations and marks a scan complete;
  an absent/pending block prevents promotion. It never writes a data apply receipt.

Known limitation: range loading currently rejects schema/table names that require
`mapping_ident` rewriting; existing full loading remains available. The SQL APIs
and real target transaction tests are delivered; whole producer -> approved
admission binary -> Rust Loader -> worker acknowledgement/publication is a separate
verification gate and is not implied by SQL fixture success.

## G2 conservative initializer limits and upgrade conditions

The layered adapter supports a previously sealed small full baseline as exactly
one unbounded active chunk. `initialize_full` and `initialize_full_if_absent`
refuse `lo`, `hi`, `chunks`, `expected_chunks` or `boundaries` with
`c5_bounded_baseline_unsupported`, before connecting. An existing bounded or
multi-chunk authority is also refused by `initialize_full_if_absent`.
Schema/table names requiring `mapping_ident` rewriting are refused with
`c5_mapped_names_unsupported`, rather than being looked up under a guessed name.
The old full import and multipart protocols are not changed by these checks.

0054 preserves 0053 and older checksums, and limits incremental C5 publication to
a real matching single-unbounded-block receipt. Bounded incremental C5 publication
is refused with OO030, including an explicit reviewed-topology-required message.
This does not restrict the old full or C4/C1 publication routes.

Do not enable bounded baselines by increasing limits or inventing initial
membership. Upgrade requires a reviewed fenced topology initializer, source-
collation block coverage, receipt-backed source digest/cursor persistence and
restart, correct native name mapping, and post-publication COPY-forward/RLS tests.
A baseline beyond one range's row/byte budget remains unsupported; it must fail
its budget without partial application. Source connection count, achieved duty
and large-table calibration are not established by the small M1 fixture.
