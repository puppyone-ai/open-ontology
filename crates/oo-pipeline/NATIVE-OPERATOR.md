# Native loader operator contract

This deployment uses one target PostgreSQL database for the authoritative
`jobs.tasks` queue and the `_oo` import transaction. A claim in a different
control database cannot authorize this loader. No receipt or lease row is
copied between databases. The old separated-control failure evidence remains.

Use the tested Linux Rust 1.94 executable recorded in the delivery report.
Source roles remain read-only; target installation uses a separate administrator.
An internal LOGIN inherits `oo_pipeline_runtime` and `oo_jobs_runtime` with
`INHERIT TRUE, SET FALSE`. It has no owner membership, superuser, database/role
creation, bypass-RLS, or replication privilege. Do not give these memberships
to the dialogue Agent or Gateway principal. Credential files remain private.

## Deploy

The installer requests use the existing private `target` object. Run
`oo-pipeline install-native-update < native-install.json` as the administrator.
Its protocol is `oo.pipeline.install-native-update/v1`; target fields are
`host`, `port`, `database`, `runtime_principal`, `password_file`, `ca_file`,
`shard`, and `owner`, using the same shape accepted by `import-native-update`.
It installs target fence/physical mapping/full-import/jsonb/native-update.
First run `oo-pipeline install-sealed-catalog < base-catalog-install.json`,
using protocol `oo.pipeline.install-sealed-catalog/v1` and the administrator
target shape below. Native catalog installation requires this 0006 base.
`oo-pipeline install-native-sealed-catalog < catalog-install.json` uses protocol
`oo.pipeline.install-native-sealed-catalog/v1`, and target fields `host`, `port`,
`database`, `administrator`, `password_file`, `ca_file`. This adds sealed catalog
functions; the source metadata catalog is not an installed or published pack.

Install the jobs schema **in that target database**:

```sh
python3 crates/oo-jobs/tools/deploy.py --service PRIVATE_LIBPQ_SERVICE --loader-role INTERNAL_LOGIN
```

For an owned isolated fixture whose administrator can use its local socket:

```sh
python3 crates/oo-jobs/tools/deploy.py --container OWN_TARGET --database OWN_DATABASE --admin OWN_ADMIN --loader-role INTERNAL_LOGIN
```

The command locks the migration executor, applies the exact source SQL, hashes
each file, and records a marker only after successful execution. Replay checks
all five digests, including `jobs-loader-hold/1`; a mismatch aborts. Cluster roles
can preexist; powerful or LOGIN preexisting jobs roles are refused. The new
`jobs.loader_hold(uuid,bigint)` SECURITY DEFINER function grants row locking
without restoring runtime direct UPDATE on `jobs.tasks`.

## Real queue and source capture

The generic production scheduler is the fixed `tools/job-worker` native service
described in `tools/job-worker/NATIVE-SCHEDULE.md`. It pins a source-owned
producer program and argv in operator configuration. Task payload is only
`{"v":1,"binding":"BINDING_UUID","pin":1}`. The producer emits the genuine
source-bound `oo.pipeline.native-update/v1` request; the worker attaches the
actual claim task UUID and generation and invokes the pinned loader. The
request cannot register its own trusted program or invent an admission bit.
Detect, pull, and reconcile periods remain source-method-specific.

For direct primitive verification, the runtime calls:

```sql
SELECT * FROM jobs.enqueue(PROJECT_UUID, 'native_capture', 1,
  PAYLOAD_JSON_TEXT, CONFIG_VERSION, ARTIFACT_SHA256, DEDUPE_KEY,
  clock_timestamp(), 3);
SELECT * FROM jobs.claim(PROJECT_UUID, ARRAY['native_capture'], WORKER_ID);
```

Use a function in FROM as shown. `(jobs.claim(...)).*` can evaluate the volatile
function several times, consuming a claim before the other fields are read.
These SQL calls verify primitives; manual SQL is not continuous product closure.

Attach `lease_id = claim.task_id` and `lease_generation = claim.generation`.
The loader locks that exact live task in the target transaction. It rechecks
the lease before commit. An absent, stale, expired, terminal, or wrong-generation
claim is refused. A long target transaction prevents a concurrent heartbeat
from acquiring that row; work must finish within the server lease or roll back.
The target-session advisory fence is additionally adopted and checked by the
existing `_oo` functions. A control claim does not certify that target fence.

## Source generations and positions

First import is source generation 1 without a parent. Next import names its
real parent generation and is exactly parent + 1. Admission captures must carry
that same generation; do not relabel a historical generation to initialize a
fresh target. The target checks the stored parent and preserves replay and
conflict behavior. Different tables need their own proper parent history.

C4/C5 full reconciliation uses `full_snapshot: true`. With
`position_codec: "snapshot-zero/v1"`, the source anchor is `applied_pos: "0"`;
the source snapshot identity stays in the sealed capture. Snapshot row counts
are not source offsets and are no longer written as the receipt/watermark
position. Snapshot replacement implements physical DELETE by omission and
does not copy deleted parent rows forward. Genuine partial delta positions
must use the source's actual supported ordered codec and lineage; never invent
an ordered offset from a row count. Parent-generation CAS still applies to
full snapshots. Oracle logs CDC remains a separate capability; C5 is not logs CDC.

## Catalog and recovery

Successful import returns `awaiting_publish` with real run_id, target_generation,
source_generation and claim observation. It does not publish. The exact sealed
catalog request uses protocol `oo.pipeline.native-sealed-catalog/v1`, `run_id`,
`source_id`, `owner`, `source_generation`, `target_generation`, `config_digest`,
`schema_digest`, `type_digest`, `input_digest`, `snapshot: {kind,value}`, and
target `{host,port,database,runtime_principal,password_file,ca_file}`. Invoke
`oo-pipeline catalog-import-native < catalog-request.json`. The function reads
the sealed target run and stored attributes, rejects identity mismatch, and
exports the Builder-compatible catalog with source type provenance.

Shadow rows, quality, apply receipt, freshness and source watermark commit
together. Control-mirror recovery is a later operation; `mirror_committed:false`
does not reverse target commit or certify mirror success. Replay verifies the
same payload and returns the existing run. The native worker retries its durable
completion/watermark state and preserves source/target generation identity.
An `awaiting_publish` catalog must then be consumed by the actual Self-build,
role-reviewed effective Ontology, Knowledge Pack and publish/Gateway workflow;
catalog export alone is not acceptance.

Native declarations preserve source type semantics: UInt64 exact numeric,
nanoseconds exact numeric, binary bytea, lists canonical JSON plus original list
type, Oracle DATE/TIMESTAMP original microseconds, and dictionaries Int32/Binary
resolved as binary. `char_length` means fixed CHAR; VARCHAR uses utf8 without
this fixed-CHAR field and retains its original size in source type metadata.
The admission validator and external driver binaries use fixed SHA allowlists
and declared exact byte budgets. Unknown or oversized programs are refused.

## Whole-source collection

After each real run exports its catalog, use the product exporter:

```sh
python3 tools/pg-builder/scripts/export_native_collection.py --operator PRIVATE_COLLECTION_CONFIG --out collection.json
```

The mode0600 operator JSON has protocol `oo.pipeline.native-collection-operator/v1`,
`pipeline: {path,sha256}`, `admission_binary`, and `entries` listing
`{request,registry,registry_sha256}` for every approved table's actual run.
It also requires `expected_datasets`, an exact list of `[logical_schema, logical_table]`
pairs from the approved source scope. The exporter refuses a missing or extra
table. Each table retains its own source identity, generations and config digest
in the binding vector; approved collections may span distinct source IDs.
Each request is the existing `oo.pipeline.native-sealed-catalog/v1` request.
The exporter invokes the existing catalog command with each operator registry
and validates its actual source/run/generation tuple. Output protocol is
`oo.pipeline.sealed-import-collection/v1`, with the untouched `entries` and a
per-dataset `binding_vector`. It creates no singleton source_id/run_id/digest.
Self-build consumes every entry and retains that vector through effective
Ontology, role Pack and Gateway target mapping. A failed entry produces no
collection and preserves the raw child failure. One table export is not a
whole-source collection.

The authoritative update trust entry is now `verify_inputs`: it resolves the
process operator registry and reruns the frozen admission validator. A request
registry_path is optional and, when present, must equal the operator path.
Caller preimage equality alone is refused. Original b593/daca lacked this
validator invocation and is retained only as component/failure evidence.
PG `seal` binds `gpt-6.1-sol` by default or the recognized actual `--config`
model before preparing a description request. Native `sealed` accepts the
existing verified `--source-contract` file for original source caveats.

The final minimal admission artifact SHA `0b584453877a4c5ec3e7c311c199ade8af723229ea2ab56745858e37f7a1e274` is explicitly approved. Per-occurrence operator registration still pins the actual registry file and frozen validator; a caller cannot select another program. For native PG UUID/text, use the source-owned registry containing the jointly observed `source_oid`, `source_typmod`, `source_format` fields. Requests cannot override those fields. Native MySQL/PG actual NativeUpdate runs use `catalog-import-native`; mature runs retain their existing catalog commands. Original quoted names are verified against stable mappings by the exporter, without editing receipt or stored names.
