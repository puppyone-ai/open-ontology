# Whole-database bounded full import

```sh
source-onboarding full --input /private/input.json \
  --operator /private/installed-operator.json --out /private/full-run
source-onboarding full-completeness --tables /private/all-tables.json \
  --ledger /private/full-run/ledger.json
```

The six public connection fields cannot choose executable paths, transport,
audit exceptions, target credentials or a manifest. Those are installed operator
configuration. `onboarding.full.batch_manifest` is an absolute private JSON file:

```json
{
  "generation": 1,
  "config_sha256": "SHA256_OF_CANONICAL_INSTALLED_OPERATOR_JSON",
  "concurrency": 1,
  "tables": [
    {"schema": "sample", "table": "items", "columns": ["id", "label"]},
    {"schema": "sample", "table": "oversized", "columns": ["id"],
     "exclusion": {"reason": "over_budget", "upgrade_condition": "Approve a sufficient budget after chunked reading is integrated"}}
  ]
}
```

Canonical operator bytes are compact sorted-key JSON (`serde_json::to_vec`).
For ordinary configuration keys, Python `json.dumps(operator, sort_keys=True,
separators=(',', ':'), ensure_ascii=False).encode()` gives the same bytes.
The operator contains the manifest **path**, not the manifest digest. Generation,
table configuration and operator digest form each ledger key; concurrency is an
execution setting, not a new data generation. Unknown/missing metrics are null.

## Installed full pipeline

Set `onboarding.full.pipeline` to activate provisioning, capture, admission,
existing job-worker/Loader, catalog export and independent source/target audit.
Without this block the existing preloaded-receipt full verifier is retained.
The full pipeline block is deliberately operator-only:

| Field | Meaning |
|---|---|
| `state_root` | Absolute private durable directory, shared at the same absolute path between host and trusted runtime |
| `jobs_deploy` | Host path to existing `crates/oo-jobs/tools/deploy.py` |
| `control_container`, `control_user` | Installed PostgreSQL container and operator principal used by the existing provisioning/export operations |
| `capture_template` | Existing `oo.mysql-full/v1` config; selected table/columns come from the manifest |
| `driver`, `admission`, `loader`, `producer_adapter` | `{path, sha256}` for mysql-preview, oo-source-admission, oo-pipeline and existing native-producer.py |
| `prepare` | Runtime path to existing mysql-preview/prepare-native.py |
| `source_audit_policy` | Explicit `grant_enforced`; never inferred from capture's audit claim |
| `target` | Existing native-update target (runtime principal, password file, CA, database, shard and owner UUID) |
| `catalog_target` | Existing sealed-catalog target using the installed catalog/operator principal; no shard/owner fields |
| `audit_target` | `{host: POSTGRES_CONTAINER, database: DB, user: READ_PRINCIPAL}` for independent SQL reads |
| `wait_seconds` | Per-table bounded worker wait, default 120 seconds; configure for source load and concurrency |

The existing `onboarding.container`, `worker`, `worker_config`, secret registry,
`host_secret_root`, `full.source_id`, `full.version`, `audit` and `product` source
access settings are also required. Runtime binaries, scripts, secret files and
durable output paths must be installed/mounted before invoking the command.
Database server/approved database and login provisioning are site prerequisites;
the command installs checksum-checked jobs, target/native/claim-selected migrations
and worker tables through the existing installers. It does not create a second
scheduler, queue, Loader or permission system.

For this grant-enforced path, `capture_template.audit` must be
`{"mode":"grant_enforced"}` and the installed `source_audit_policy` must separately
be `grant_enforced`. Explicit plaintext is selected only in the installed capture
connection (`tls:"disabled", plaintext_authorized:true, ca_file:null`); the real
driver still requires actual `have_ssl=DISABLED`. For network-mode independent
source SQL, the existing installed `full.source_transport` policy must also be
configured. There is no request-controlled TLS fallback. Endpoint/account/secret
bindings must match the installed six-field connection.

A table may override `budgets` with the existing capture-full budget object.
Chunk-parameter references can be carried in table objects to the replaceable
`batch::run` callback. Tables that overflow their row/byte budget, require charset
conversion, or have composite keys use capture-full chunks and the C5 multipart
Loader. Optional table `chunking: true` forces that path. Ordinary bounded small
tables retain their single-request shape. The ledger adds `parts`; `chunks`,
`rows`, Arrow IPC `bytes`, `elapsed_ms` and independent `comparison` are retained.
Multipart admission uses a bounded anchor plus the complete sealed part manifest;
it does not read the combined Arrow file or raise its old 32 MiB cap.

`pipeline.runtime_transport: "host"` optionally runs the same pinned worker and
Loader on the operator host; absent/`"container"` keeps the existing Docker runtime.
The six public fields cannot select this. TCP still uses the existing caller-CA
TLS policy. No job claim, fence, admission or publication check is bypassed.

Source conversion/budget failures are forwarded as rejected C5 occurrences: the
worker uses its real lease to persist only masked records in `sync.rejects`, then
fails/cancels the occurrence without advancing the data generation. The rejected
capture is never a source-bound admission receipt.

Independent multipart comparison reads each source range using its own key
collation and compares typed rows with the target by exact captured key membership,
not target-side string ordering. Changed source blocks are recorded unresolved
with a null difference conclusion and fail the audit. Capture Arrow cells are used
only to distinguish a changing source from a target mismatch, not as the sole
source/target equality proof.

## Execution, recovery and failure isolation

Tables are grouped into batches of **at most 32**; default concurrency is 1,
configurable from 1 through 32. No existing C1/v2 hard limit is increased.
A deterministic per-table/configuration binding registers the existing C5 native
producer and Loader. Real worker fire/claim/heartbeat/Loader/complete-success CAS
is authoritative. The producer publishes actual capture/admission artifacts;
the worker alone invokes Loader. Every table keeps the same actual source ID;
source-level native fencing can serialize target application even when callers
configure parallel preparation/audits.

Each successful import exports the committed worker request/result and invokes
the existing sealed catalog command. The existing single-table `mysql::full`
then calls `audit::compare_full`: independent frozen-source and target queries,
full selected typed-cell equality, including zero-row tables. Its receipt and
oracle are retained under the attempt's `verified/` directory. Ledger success
requires these checks, not just producer hashes, row counts or a Loader exit.

`ledger.json` is atomically replaced after file and directory sync. An OS lock
rejects concurrent batch runners and is released on process death. Records have
pending/running/succeeded/failed/excluded state, batch index, rows/chunks/bytes,
elapsed time, comparison and failure. Attempts/failure history are retained.
Successful/excluded current keys are skipped; incomplete attempts get a fresh
attempt directory. Real worker state and durable producer occurrences handle a
kill between Loader commit and ledger persistence without another COPY/capture.
No lease, successful task, admission bit or watermark is fabricated by this tool.

A rejected producer fails the table and cancels only its queued C5 occurrence
through existing `native-cancel`, including the existing target-not-applied checks.
This releases the real source fence so other tables can run. Failed capture
attempts restart the whole table. Explicit independent comparison failures mark
the durable audit cycle invalid; the next attempt creates a new full occurrence,
not a partial Arrow continuation. An interrupted (not explicitly failed) audit
can reuse the already committed load and rerun the independent comparison.

The only exclusions are `empty_without_key`, `unmappable_type`, and `over_budget`.
Each requires a nonblank `upgrade_condition` and operator/human review; these are
not automatic claims that excluded data was imported. Completeness names missing
conclusions and unjustified exclusions. A failed table with a reason counts as a
conclusion, **not success**; the full command returns nonzero if selected current
keys failed. Use the entire source inventory as the completeness input. Checking
a partial inventory cannot discover tables omitted from that inventory.

## Source-statement acceptance

Successful runs may emit only SELECT/SET/START TRANSACTION/COMMIT/SHOW and the
existing read-only transaction statements. **Successful runs must have zero KILL.**
A failed/rejected candidate is wholly invalid. Its cleanup may issue
`KILL CONNECTION` **only for its own failed source connections**, preserving
bounded cancellation without changing source data. This exception is not a
permission to kill another connection or to accept a partial candidate.

`testing/mysql-full-c5/statement_review.py` separately checks successful source
statement verbs/zero KILL and failed cleanup targets against actual captured
connection identities. Any DML/DDL, unknown verb or unrelated KILL is rejected.
A fixture administrator may enable general_log solely to corroborate a separate
run; the product never enables it. Grant-enforced evidence remains a lower-grade
explicit exception and must not be described as source-audited zero-write proof.

Ledger resume assumes the same durable target and worker database still exist.
A completed ledger is not a target-availability certificate. Do not reuse its
configuration/generation after replacing or resetting the target database; recover
the native durable state together, or start a new generation/configuration.
