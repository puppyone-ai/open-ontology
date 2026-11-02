# Oracle integration producer contract, 2026-10-27

This is the source owner's concrete input to the ongoing shared typed/v3 work.
Root/MySQL owner owns pipeline, worker, registration, materialization, publication
and control CAS. Oracle is never registered as mysql.

Current production integration mode is **immutable buffer consumption**, per the
coordinator's checkpoint decision. See `BUFFER-CONSUMPTION-CONTRACT.md`. Do not
require/synthesize a vendor offset for each captured transaction. The retained
Debezium final offset/history remain source-resume artifacts, independent of
target consumption ACK. Per-transaction vendor-offset export, when real pairs
exist, is a separate optional mode and is not used by the retained buffer input.

## Immediate argv

```sh
oo-oracle-source < request.json
python3 product/tools/oracle-preview/producer.py bundle --config EXPORT.json --out NEW_BUNDLE
python3 product/tools/oracle-preview/producer.py validate --bundle NEW_BUNDLE --sha256 MANIFEST_SHA
oo-oracle-source c1-baseline-stream-v3 --config /absolute/ARGV.json --out /absolute/NEW_RUN
```

`oo-oracle-source` is the root-built optional main-crate binary (original fixed SHA
`abbee41dbea4114ced9468a7cb4510083f72e43afb18feba61a894daa8e14f1f`).
The focused `oracle-chunk-reader` remains a test harness, not the product entry.
See `ROOT-ORACLE-FEATURE-REQUEST.md` for root's completed feature/binary check:
default excludes Oracle; the Oracle feature uses product Arrow 60, not driver
Arrow 59. Local-only and type/inspection budget guards remain unchanged.

### Server version preflight (2026-10-28)

The same native binary accepts `{"action":"server-metadata","config":CONFIG}`.
This action was first live-verified in binary
`a2319724509ccc6dbcc500750ac9b3a826002170bbf58144d2b0ce6e7668c3f2`;
earlier capture binaries need not implement it. Existing capture pins are not
changed by running this separate preflight.
CONFIG needs the existing `host`, `port`, `user`, `service`, and private
`password_file` fields; no owner/table or pre-known database identity is needed.
Use the existing `bounded_source.Driver.call("server-metadata")` parent timeout.
The loopback-only guard remains: this does not authorize a new customer route.

This action issues fixed metadata SELECTs only, at most eight rows per query
and 64 KiB of decoded values, with a five-second per-call timeout. It does not
run session ALTER statements, inventory, business SELECTs, capture or writes.
Read `server_version_identified`, not just the process exit code. The reported
version comes from server `product_component_version`, preferring VERSION_FULL;
missing old-server columns are retained as failures before a bounded legacy
query. Both Oracle Database and Oracle AI Database names are recognized; client
or SQL*Plus versions are never substituted. Patch inventory remains unmeasured.

The owned 23.26.3.0.0 server was actually tested. Older catalog shapes have unit
coverage only, not actual older-server driver compatibility. Customer version,
credentials, route authorization and edition/license acceptance remain separate.
Do not use historical `real_probe.py` for this preflight: it also samples rows.

### EXPORT.json

Protocol `oo.oracle.arrow-producer-config/v1`, engine `oracle`, `source_id`,
hash-pinned `reader: {path, sha256}`, `chunk_rows` in 1..4096.

- `mode=sealed_snapshot`: `snapshot: {path: absolute manifest.json, sha256}`.
  All original parts/journal/manifest hashes are checked. Every original part is
  exported; unsupported types refuse the bundle rather than silently project.
- `mode=debezium`: hash-pinned offline SQLite backup in `spool`, pinned
  `schema_manifest` for the same source ID/identity, exact `table` (owner.name),
  `connector_offsets`, `connector_history`. Live WAL files are refused.
  Snapshot first/true/last, unique baseline keys, schema, complete END/contiguous
  event order, commit SCN and full before images are checked. A separate SQLite
  operation journal replays the committed data. No source/network/model is called.
- `temporal_policy: {"timezone":"utc_instant"}` is necessary for TSTZ conversion.
  It explicitly normalizes to the exact instant, not original offset/region text.

The source identity associated with a retained spool is supplied by its pinned
native schema/independent-typed provenance. It is still subject to official
source admission; bundle generation is not permission/driver certification.

### Produced bundle

`oo.oracle.producer-bundle/v1`: source/reader/producer/config hashes, source
boundary, `typed_tables`, source mappings, checkpoints, batch list, all artifact
hashes and the independent operation journal. `target_apply/admitted/published`
remain false until the corresponding real shared stages return their receipts.

`batches.jsonl` carries **existing** `oo.c1.typed-batch/v3` objects. Fill is one
part/batch; a complete transaction is one CDC batch. Each table has one IPC
stream, with many bounded record batches if necessary. It is never split into
separately committed pseudo-transactions. Each Arrow part retains its existing
100k-row / 32 MiB resource bounds; there is no total part-count or aggregate-row
ceiling in typed/v3 batch validation. Capture positions are immutable sequence 0,1,..., NOT SCNs
or GTIDs. Each sequence binds tx ID, commit SCN, event count and payload SHA.
Only the final cut binds the opaque Debezium offset; intermediate recovery is
from retained capture, never an invented per-transaction connector offset.
The current shared `copy_bytes` rejects repeated keys in one transaction part.
This producer explicitly refuses `repeated_key_transaction_coalescing_required`;
it does not silently discard intermediate log rows. PK-changing updates also
require a separate before/after-key contract. This is not universal Oracle CDC
transaction-shape support.

## Precise temporal target mapping

- Oracle DATE -> Arrow Timestamp(us, no TZ) -> PostgreSQL timestamp, OID 1114.
  Time of day is preserved; DATE is not reduced to date32.
- Oracle TIMESTAMP -> Timestamp(us, no TZ) -> OID 1114. No timezone is invented.
- TIMESTAMP WITH TIME ZONE -> Timestamp(us, UTC) -> OID 1184 only with the explicit
  `utc_instant` policy. Original region/offset representation is not retained.
- All parsed nanoseconds must be divisible by 1000. For TIMESTAMP(7..9), exact
  microsecond-aligned values can be represented; any nonzero submicrosecond
  digits produce `submicrosecond_target_contract_required`. No truncation/rounding.
- BC era, invalid dates, leap seconds, local-time-zone type and unagreed temporal
  mappings refuse. AD 1..9999, Gregorian leap years, pre-epoch values and signed
  offsets (including -00:30) are checked without float arithmetic.
- Fixed NUMBER/RAW/text retain native Arrow numeric/binary/text mappings.
  Unconstrained/negative-scale NUMBER remains explicitly unadmitted to the target
  encoder. No fallback TEXT is described as native numeric/time support.

## Worker input/ACK adapter needed from shared owner

This is the existing worker's five-argument producer layout; no shell wrapper or
extra argv prefix is needed. The native binary verifies `python` and `helper`
objects (`{path,sha256}`) and the adjacent `bounded_source` dependency, then runs the pinned standard-library-only Python
helper's `serve --config ... --out ...` path. The helper copies bounded Arrow
parts into the new `--out/parts` directory so the worker's configured-out guard
remains intact; replay digests do not include transport paths.

ARGV config protocol `oo.oracle.producer-argv/v1`, engine `oracle`, pinned
`python`, `helper`, `bounded_source`, `bundle` manifest, normal source-admission config, and root-supplied **official**
`materializer: {path,sha256,args}`. Missing materializer refuses **before open**.
For the agreed buffer mode, also supply pinned `buffer_cursor` (the adjacent
`buffer_cursor.py`) and `buffer_index`. The helper verifies every source event/
END against the immutable spool and binds the index to this exact bundle before
emitting frames. The same mainline binary runs this hash-pinned helper.

Bootstrap protocol `oo.jobs.oracle-c1-bootstrap/v1`, engine/source_id,
source_generation, lease_id, lease_generation, binding/pin/target as needed by
the common materializer. No mysql UUID is synthesized. Previous/resume remain
explicitly refused until the shared recovery adapter is supplied.

Frames: `open` -> `stream_started` -> `typed_batch` (fill) -> `snapshot_end` ->
`typed_batch` (complete CDC txn) -> `end` -> `materialized`.
In buffer mode Open carries the structured actual source identity and measured
ID `oracle.<DBID>.<RESETLOGS>.<lowercase PDB GUID>`, matching the implemented
shared registry, plus `checkpoint_kind=buffer_consumption` and the context ref,
`position_codec=oracle-capture-sequence/v1`, `l0_pos=0`, source snapshot SCN,
schema preimage/digest and real `typed_tables`. The zero is a capture cursor,
not an Oracle log SCN; source epoch comes only from the target's real open ACK.
L0/Lend are explicitly `oracle-buffer:<context SHA>:<ordinal>` handles, not
vendor offsets. Every CDC frame adds the independently tagged buffer position.

Worker ACK protocol: `oo.jobs.oracle-c1-baseline-ack/v1`, exact stage, durable=true.
For typed batches, nested receipt must be `oo.c1.typed-ack/v3` and match source,
real task/lease generation/epoch, sequence, position and the existing path-free
`Batch::digest()` (UTF-8 serde-compatible canonical JSON). A target-only receipt
or timeout does NOT advance the producer.
CDC buffer ACK additionally requires top-level fields in the typed `receipt`:
`checkpoint_kind=buffer_consumption`, exact context/bundle SHA, `buffer_cursor`,
`source_evidence_sha256` and `source_offset_write=false`. The old proposed
`receipt.buffer_checkpoint` shape is not the implemented worker contract.
The committed `source_position.buffer_position` must match the input position
with only artifact paths removed; the payload hash still includes those paths.
The source owner emits no
ACK on behalf of the target; simulated-peer tests remain protocol-only evidence.

After a genuine `end` ACK, invoke the pinned common materializer on stdin:
`oo.oracle.materialize-input/v1` with original context, source epoch, pinned
bundle, sealed target receipt, L0/Lend and checkpoint. Worker replies are capped
at 1 MiB: the production `end` ACK must carry a hash-pinned target export path or
bounded part manifest, **not an unbounded whole-table rows array**. Target export
and final Arrow materialization remain the common Loader owner's responsibility.
Required response:
`oo.oracle.materialize-result/v1`, engine/source_id and `request` containing the
real official admitted native request. Only then emit `materialized`.
Do not substitute a source-generated successful request or fill fake provenance.

The shared Loader now implements `native-oracle-c1-materialize`. Use the fixed
pins/config in `FORMAL-TAIL-CONTRACT.md`, not a callback shim. The producer checks
pinned registry/envelope and schema/type/config digests before returning the
materialized request; a synthetic successful callback is explicitly rejected.
Actual admission/finalseal/CAS/publication acceptance remains a separate real run.

## Source-onboarding boundary

The original `six_fields()` and installed `/etc/open-ontology/native/mysql-operators.json`
are MySQL-specific. Root owns the explicit Oracle operator/branch. Oracle input
has exactly `type,host,port,service,database,user,secret_ref`; database is expected
DB_UNIQUE_NAME, not service/PDB or owner. Source identity, approved table scope,
native producer/bundle/driver pins, type policy and declared budgets belong in
the installed operator. Do not
relax the existing MySQL admission guard or silently reuse the old 4-row Oracle
customer profile. Local source CLI remains loopback-only in this authorized phase.
`oracle_operator.py` creates measured local source registrations; worker/target
registration and product runtime remain root/shared-owner authority.

No business dictionary/real S5/semantic_scope/model/Skill approval is created by
the producer. The frozen 4000I/4000U/2000D journal belongs to the upcoming joint
target chain; retained 3000/4000/3000 extended evidence is not renamed.

## Temporal CDC wire contract

The producer now validates each temporal column against the actual `before` or
`after` Connect schema, including NULL-valued fields. It does not infer epoch
units from a value's magnitude or accept a timestamp-shaped string without its
logical type. These mappings follow the tagged Debezium 3.5.2.Final Oracle
connector contract, the version observed in retained actual snapshot envelopes:

| Oracle type | Wire type / logical name | Exact target |
| --- | --- | --- |
| DATE | int64 / io.debezium.time.Timestamp, milliseconds | timestamp, whole seconds |
| TIMESTAMP(0-3) | int64 / io.debezium.time.Timestamp, milliseconds | timestamp, declared precision |
| TIMESTAMP(4-6) | int64 / io.debezium.time.MicroTimestamp, microseconds | timestamp |
| TIMESTAMP(7-9) | int64 / io.debezium.time.NanoTimestamp, nanoseconds | timestamp only when submicroseconds are zero |
| TIMESTAMP(n) WITH TIME ZONE | string / io.debezium.time.ZonedTimestamp | timestamptz with explicit utc_instant policy |

Connect's millisecond Timestamp logical type is accepted only for DATE or
TIMESTAMP(0-3). It is refused for higher precision even if a particular value
looks aligned; a lossy converter is not an exact source contract. Floats,
integer strings, mismatched/duplicate/missing field schemas, out-of-range
values and nonzero submicroseconds fail closed. LOCAL TIME ZONE remains
unsupported until its session/type semantics have a verified contract.

Civil timestamps do not acquire an invented timezone. Zoned values use an
explicit offset and normalize to the exact UTC instant; native snapshot and
CDC before-image journals use the same representation. Raw captured events,
event counts, schema hashes and source files are not rewritten. Original
region/offset representation is not retained under the existing utc_instant
policy.

`test_temporal_producer.py` covers exact/negative epochs, NULLs, precision and
wire-schema refusals, timezone equivalence, real offline Arrow encoding and
buffer rollover. Offline codec results are not a real Oracle CDC/target/Gateway
acceptance claim; live source, publication and question receipts remain required.

## Explicit multi-table transactions

Captured-buffer producers and continuous capture accept either legacy
`"table": "OWNER.TABLE"` or `"tables": ["OWNER.A", "OWNER.B"]`, never both.
Lists are nonempty and contain distinct exact tables under one owner, without
a source-wide table-count ceiling. Wildcards, regexes,
ambiguous fields, missing members and silent membership changes are refused.
Continuous capture verifies the identity, object, keys and columns of every
member against the retained seed before starting; the connector filter is the
same explicit sorted membership.

Producer bundle/serve and managed-capture configuration files are read with a
16 MiB byte budget before JSON parsing. Existing manifest/schema, spool,
record-batch, row, transport and deadline budgets remain enforced. An oversized
input refuses as a whole; it does not keep a prefix of the declared tables.
Neither the explicit list nor the source schema/digest is sliced into 32-table
subsets.

Source membership and transaction capacity are different boundaries. An initial
fill for 33 tables uses the existing one-table-per-fill-batch path. A CDC
transaction across a subset of those tables still emits one complete atomic
batch. A transaction touching all 33 tables remains one ordinal, one batch,
one target transaction and one complete receipt. The producer does not apply
a total-part or total-event-count ceiling. Input/spool budgets and per-part
encoder limits still fail closed rather than truncate or split a transaction.

The typed target validates all part descriptors (including duplicate table
refusal) before apply, then reads, hashes, decodes, validates and COPYs one part
at a time inside the same target transaction. Only one part's COPY payload is
retained at a time. A bad later part rolls back all prior parts, tombstones and
receipt work. Even a durable receipt replay revalidates every supplied part.

An initial Debezium snapshot validates per-table wire schemas and a common SCN,
including FIRST/LAST and collection-boundary markers. A one-row snapshot may
carry only LAST, matching the vendor marker precedence. Every selected table
must have actual snapshot evidence: an absent or empty member is not silently
dropped or fabricated. Native fixed-SCN snapshot/resume proof remains limited
to its separately verified single-table contract.

All events in one committed transaction are kept under one ordinal and one
typed batch. That batch has one Arrow part per changed table, preserving
within-table event order; distinct tables with identical keys never collide.
Repeated-key transactions retain the explicit ordered-batch protocol. Raw END
counts and per-collection counts must agree with the Arrow rows for each table,
not only the grand total. Both Python buffer verification and Rust target
admission enforce this partition binding before apply.

Rollover replays and compares the journal of every admitted table. Schema,
membership, opaque checkpoint history and committed-prefix guards remain in
force. The existing typed target transaction applies all parts atomically;
per-table ACKs are not substituted for the transaction checkpoint.

Tests exercise complete 33-table fill/schema/digest/buffer membership, a
three-table I/U/D and single 33-part transaction inside that scope, refusal of a missing/foreign
33rd member, per-part resource guards, input byte bounds,
two-table updates, interleaved repeated-key operations, exact
event partition counts, missing/foreign tables, collection markers, metadata
drift and retained-buffer rollover with the real offline Arrow codec. Unit tests
alone do not establish actual LogMiner, recovery, publication or business-answer
acceptance.

The original source-only handoff did not remove downstream collection ceilings.
The continuation removes Loader registration/fill/final import/publication
collection ceilings and the rollover total-table/journal-row guards. Additive
migration 0109 replaces only total-cardinality checks in the current publication,
library, initial-fill and buffer-record functions; old migration files and
checksums remain unchanged. It is installed by the ordinary
`install-native-c1-typed` dependency closure. Worker catalog/entry changes are
supplied separately by the coordinator. Source bundle success is never reported
as target ACK or publication success. The owned 33-table evidence and
the shared-interface history live under
`coordinator/oracle-33table-continuation-20261029`.

The owned Oracle Free continuation now records a real three-table I/U/D followed
by one 33-table UPDATE transaction, ordinary worker ACK/publication through gen3,
and all-table source/target/independent-gold comparisons with zero differences.
Its bad-final-part attempt executed 32 COPY/apply operations before a digest
refusal; backend disconnect aborted the open transaction without a receipt or
checkpoint advance. Normal same-task retry committed all 33 parts together.
See `chain/wide/atomic-proof/receipt.json` and `chain/wide/audit/receipt.json`;
these are not customer-version or whole-L3 certification. The optional
Pack/Gateway/Haiku path stopped at `s5_total_token_estimate_budget`, before any
model call.

The separate legacy vendor-position validators in `buffer_cursor.py` and
`c1_source_position.rs` still impose a 100k event-count guard. They are outside
this assigned change and do not block this 33-event proof; arbitrary larger
source transactions are not claimed to be accepted end to end.
