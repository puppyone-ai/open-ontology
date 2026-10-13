# SQL Server original09 full-source operator contract

Status on 2026-10-12: **pending real source**, not ready or accepted. Existing
reader source is fixed at `1449ecc5cd03e05bc82c910e98264d05dfe61d3c`.
No SQL Server source has been started and this task has not accepted the EULA.
The host Docker platform remains Linux arm64; x86 emulation is not a native
SQL Server proof. Do not replace SQL Server with another engine.

## Required operator input

Supply an approved native Linux amd64 SQL Server 2022 endpoint, license/EULA
authorization, TLS CA trusted by the reader, approved objects, and a private
`oo.mssql-preview.customer/v1` JSON file. Its `secret_ref` is an absolute path
to a mode-0600 reader password file. Credentials must not enter argv, logs,
Git, evidence, or model input. The DBA identity is distinct and operates only
the approved synthetic fixture namespace. Customer databases remain zero-write.

The JSON fields are `protocol`, `source_id`, `secret_ref`, `endpoint`
(`host`, `port`, `database`, `username`), `encrypt: required`,
`trust_server_certificate: false`, optional absolute `ca_file`,
`approved_objects: [{schema, name}]`, `budget`
(`unit: Rows`, `hard_limit`, `confidence: declared`, optional `requested_rows`),
and `permissions_expected: [SELECT]`. Deadline is an absolute future Unix ms.
Reader grants must lack INSERT/UPDATE/DELETE/ALTER. Inline passwords are refused.

## Implemented commands, run on the approved native runner

Build the unchanged connector with Linux Rust 1.94.0 and its checked-in lock:

```sh
cargo build --locked --manifest-path tools/mssql-preview/Cargo.toml
```

Set paths only (no secret value); replace `/private/operator` with the real
approved private directory and use an absolute binary path:

```sh
export OO_MSSQL_CUSTOMER_CONFIG=/private/operator/customer.json
export OO_MSSQL_CATALOG_OUT=/private/operator/catalog.json
/absolute/path/mssql-preview preflight --config "$OO_MSSQL_CUSTOMER_CONFIG"
sh tools/mssql-preview/scripts/run-external.sh /absolute/path/mssql-preview
/absolute/path/mssql-preview load-arrow --config "$OO_MSSQL_CUSTOMER_CONFIG" \
  --object dbo.orders --out /private/operator/orders.arrow
/absolute/path/mssql-preview gates
```

Repeat `load-arrow` for each explicitly approved object. This is a bounded
`SELECT TOP (n)` preview, without ordered full chunk enumeration or stable
snapshot proof. Do not use it as a full unsampled import/capture oracle.
`gates` must currently exit 5; `seal-request` must currently exit 4.
No wrapper may convert these failures into source acceptance.

## Full-chain handoff inputs and exits

1. **Native admission/loader owners 02/79** receive live server version and
   edition, observed login rights, approved object metadata, exact Arrow/hash,
   original SQLServer type semantics and independently observed audit/cancel.
   They must register `sqlserver` as its own engine and return actual admitted
   local catalog/table receipts. Old adapter pins are admission `4055237`,
   catalog `a6825165`, semantics `29aae36b`, consumer `6d9d6cbb`; these do not
   support the new full-source loop. Do not self-issue a seal or edit their code.
2. **Capture owner 74** receives explicit C1 CDC or C2 CT capability selected
   after a DBA setup receipt. C1 requires DBP2 transaction/LSN and replica proof;
   C2 requires CT minimum-valid-version retention checks, snapshot-consistent
   key/delete reconciliation and persisted version recovery. A rowversion cursor
   requires the documented MIN_ACTIVE_ROWVERSION upper-bound test. Platform
   must not enable CDC/CT. Existing SQL guard rejects CHANGETABLE; no C1/C2
   adapter exists here yet. Configurable detect/pull/reconcile intervals, real
   INSERT/UPDATE/physical DELETE, stop-write catch-up, restart/replay, expired
   position/schema failure and no stale resurrection need actual source tests.
3. **Self-build/Agent owner 71** receives approved classified structure/rules
   and actual local catalog identity, not raw customer samples. Run actual
   S0–S10 with the approved proxy/key and gpt-6.1-sol/low; preserve effective
   `ontology.yaml`, generated/curated/reviews, role classification and cache/
   schema-change maintenance receipts. No handwritten successful phase receipt.
4. **Publish owner 73** consumes that real reviewed effective ontology and the
   same source/data/semantic/role versions to publish and compile a real Pack.
   No fixture Pack or dummy lease/fence/catalog is an acceptable substitute.
5. **Gateway/CLI fixed def424** and owner 71's independent Agent consume the
   real Pack via public knowledge/describe/validate/query/freshness. Agent
   autonomously selects tools across multiple real turns. Compare every full
   typed expected cell independently; bind results and answers to audit query_id,
   source/Pack versions and full-role denial tests. MCP/export availability must
   be stated from actual implementation, with mandatory later SP exits retained.

Stages 1–5 have no runnable complete SQLServer interface today. Their exact
fixed API/commands must be supplied by the sole shared writers before running;
this document intentionally contains no invented successful API calls. Native
source absence and missing C1/C2/full enumeration remain separate blockers.
The original DBX-W08 AT-SRC-01/03/08 and AT-PL-04, Azure/RDS variants, 14-day
nightly, customer/license/release conditions all remain unpassed.

## Native C2 reader continuation

The former TOP-only implementation gap is addressed by `snapshot-full` and `ct-pull`.
The old section above records the interim71971 state; it is not the current reader
capability boundary. Both commands perform actual TDS, complete COUNT_BIG-checked
chunk enumeration under one SNAPSHOT, with original types/PKs and CT anchors. They
require a real pre-enabled CT/snapshot capability. Old `load-arrow` remains a preview.

```sh
/absolute/path/mssql-preview snapshot-full --config "$OO_MSSQL_CUSTOMER_CONFIG" \
  --object dbo.oracle_row --chunk-rows 1000 --out /private/operator/snapshot-1
/absolute/path/mssql-preview ct-pull --config "$OO_MSSQL_CUSTOMER_CONFIG" \
  --object dbo.oracle_row --position /private/operator/actual-target-ack-position.json \
  --chunk-rows 1000 --out /private/operator/ct-2
```

Set explicit ChangeTracking capability and an appropriate aggregate row cap and
optional `budget.timeout_ms` before reading. Source stage emits a candidate only;
never copy it into an applied-position file before actual target commit/CAS. A full
reconcile can use snapshot-full even after an applied generation. Native consumer
refuses nonbinary text-key equality until exact collation support exists, preserving
those bytes and semantics in source artifacts. Binary(4) original PK is supported
with canonical `\\x` lowerhex bytea identity, not replaced by the unique integer id.

Fixture DBA may manually use 06-change-tracking.sql before 04-load.sql, then run
07-change-window.sql after paused/no-active schedule. These files are never run by
the product reader; no script in this task accepts EULA or starts SQLServer. Obtain
an explicit DBA provenance receipt for the exact database/table, risk and rollback;
source producer combines that input with actual observed CT facts, not a fabricated
DBA gate. Separate audit observer identity reads preinstalled SQL Server Audit via
`audit-export`; deny any product-reader write attempt including rejected writes.

`tools/mssql-preview/scripts/native-producer.py --context PRIVATE_CONTEXT` accepts
one worker `oo.jobs.native-producer/v1` JSON on stdin and outputs only the real native
request. Context protocol `oo.mssql-preview.native-producer-context/v1` fields:
`binding`, `customer_config`, `audit_config`, `audit_file_glob`, `artifact_root`,
`object: {schema,name}`, `dba_provenance_file`, `target` (actual loader target),
`worker_config`, `reader_binary`, `worker_binary`, `admission_binary` (each
`{path,sha256}`), optional `chunk_rows`, `max_arrow_bytes`, `reconcile_every`.
No context contains password values. DBA provenance protocol is
`oo.mssql-preview.dba-provenance/v1` with exact `database`, `object`, `created_by:dba`,
`risk`, `rollback`, supplied from the actual setup/operator record.

Use one binding for the original source/table, coordinating detect/pull and periodic
full reconcile (every configurable N successful captures). Worker period_ms is the
combined CT detect/pull interval. Multiple independently racing cycle bindings on
one source watermark are not supported. Worker registration uses the same exact
admission path/hash plus `registry_path: ARTIFACT_ROOT/{binding}/pin-{pin}/registry.json`.
The immutable pin cache is an actual captured request/registry, not a success marker.
Only worker's actual target CAS advances state. Producer retries preserve raw failures
and reuse the exact sealed immutable pin input; context changes mid-pin are refused.

Shared interfaces required: worker f0076ce native-state/native schedule; current
SQLServer/R1/C2/file-reference-capable admission02; native loader79 with bytea PK,
`mssql-ct-version/v1` anchor (no per-row increment), typed empty stream and declared
max_arrow_bytes. Source-independent Self-build/control/publish/Gateway use the
actual sealed native catalog and original Arrow metadata. Current pipeline operation
is `oo-pipeline import-native-update`, actual lease supplied exclusively by worker.
S0–S10 and publication/Pack/actual Agent calls must be run against its real resulting
local catalog; no operator-written Ready/lease/fence/published files are permitted.
No real source/whole-loop execution has occurred on this host.

CT algorithm follows Microsoft primary documentation:
[Work with Change Tracking](https://learn.microsoft.com/en-us/sql/relational-databases/track-changes/work-with-change-tracking-sql-server?view=sql-server-ver17)
and [database recovery identity](https://learn.microsoft.com/en-us/sql/relational-databases/system-catalog-views/sys-database-recovery-status-transact-sql?view=sql-server-ver17).
