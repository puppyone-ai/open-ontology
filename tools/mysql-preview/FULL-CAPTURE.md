# MySQL no-binlog full/C5 capture

`capture-full` is a product source-only entry, separate from the existing owned-fixture `capture-native`, customer demonstration, and C1 commands. It requires the connector reader change exporting `FullReader::read_on` and `full_read_grants`; the pinned baseline alone does not have those APIs. It does not import, publish, register identities, or change source settings.

## Input

Private `capture.json` (anonymous names; replace paths with controlled secret mounts):

```json
{
  "protocol": "oo.mysql-full/v1",
  "source_id": "warehouse",
  "generation": 1,
  "owner": "00000000-0000-4000-8000-000000000001",
  "secret_ref": "secret://warehouse/mysql-reader",
  "connection": {
    "host": "mysql.internal",
    "port": 3306,
    "database": "warehouse",
    "user": "source_reader",
    "secret_file": "/run/secrets/mysql-reader",
    "tls": "disabled",
    "ca_file": null,
    "plaintext_authorized": true
  },
  "audit": {
    "user": "source_reader",
    "secret_ref": "secret://warehouse/mysql-reader",
    "secret_file": "/run/secrets/mysql-reader"
  },
  "approved_objects": [
    {"schema": "warehouse", "table": "depots", "columns": ["id", "label"]}
  ],
  "table": {"schema": "warehouse", "table": "depots", "columns": ["id", "label"]},
  "budgets": {
    "connect_timeout_ms": 5000,
    "statement_timeout_ms": 10000,
    "deadline_ms": 30000,
    "max_rows": 10000,
    "max_bytes": 8388608
  }
}
```

Exactly one selected table must match an approved object, including its column list; an empty list means the whole table and must be explicitly approved as such. Source credentials are resolved by the caller to absolute private regular secret files; this tool reads those files, not a new secret registry. No password belongs in JSON, argv, logs, or Git. `generation` is positive; `owner` is a fresh run UUID. The worker producer replaces these with the next acknowledged generation and a fresh run UUID.

For verified TLS, use `"tls":"required"`, `"plaintext_authorized":false`, and an absolute `ca_file`. There is no automatic downgrade. Explicit disabled TLS is accepted only when the actual source reports `have_ssl=DISABLED` and an empty cipher. It is **not encrypted**; SSH does not prove an encrypted remote tail.

The source must be MySQL with an InnoDB base table and the supported type/session mapping. Explicit C5 full capture accepts binary logging either off or on: enabling logs must not disable an existing read-only C5 service before its separate C1 handoff is accepted. Single integer/varchar and composite non-null keys are supported; without a primary key, capture selects the first non-null unique index. Without either, only a bounded single full read is allowed. See [CHUNKING.md](CHUNKING.md) for the stable API, legacy charset conversion, rejects, budgets and resume contract. SELECT is required; observed USAGE, PROCESS, LOCK TABLES, SHOW VIEW, REPLICATION SLAVE, and REPLICATION CLIENT are classified without using locks or replication. Writes, unknown privileges, and delegation remain refused. No binlog, GTID, C1, T grade or synthetic source cursor is created. Keyset checkpoints are local capture artifacts, not source log positions.

## Source audit and failure boundary

The observer uses a separate connection and must already be authorized to read `mysql.general_log`, and source logging must already include TABLE. The audit user/SecretRef/file may equal the source reader's; the business connection still passes the same read-only grant checks. A separate account is optional, not a requirement. Capture never enables/configures audit. It binds source records to the actual business connection ID and source-clock lower bound, requires Connect/start/select/COMMIT and zero forbidden statements, and checks logging enabled before and after. Records, not client statement notes, are embedded in the captured witness and independently checked by admission. Missing, wrong-connection, write-bearing, unknown-command, or incomplete records refuse the candidate. Alternatively, capture may use `"audit":{"mode":"grant_enforced"}` when the operator has explicitly installed that admission policy. Pass `--source-audit-policy grant_enforced` to prepare-native.py, or set `source_audit_policy` in the operator-owned native-producer config. Neither the capture witness nor observed grants enable the admission policy by themselves. Chunk audit envelopes retain separate complete evidence for every successful business connection. External audit integrations still require an explicit adapter, not a caller `writes=0` assertion.

The deadline bounds connection, access checks, read, audit and capture. Existing
single-read caps remain 1,000,000 rows, 32 MiB requested bytes, 16 MiB actual
single-batch raw cap, 300 seconds total and 30 seconds per statement. Final IPC
must fit the byte budget. Keyed raw/IPC overflow automatically switches to
chunks. Optional `"chunking":true` explicitly requests chunks; it also permits a
configured total deadline up to 24 hours, while each range remains bounded by
the statement limit. Per-range caps apply, not a cumulative table row limit.
Chunked tables are B, never S0; successful existing single snapshots retain S0.

On error/deadline, only actual owned IDs are used for bounded same-user KILL
cleanup (including the observer). This is permitted failure cleanup, not a
successful read statement or a source business write. Cleanup has a separate
5-second cap per connection; no valid capturer is emitted on failure.
`failure.json` retains the legacy candidate-invalid/restart marker. For a
chunked candidate, rerunning the identical config/output can still reuse only
completed chunks after identity/schema/collation and SHA-256 validation; never
reuse an incomplete or corrupt Arrow. A new output is required for single-read
retries or changed configuration/schema. Worker producer attempts currently
still use fresh directories; direct capture-full supports checkpoint resume.


## Capture, admission, worker producer

```sh
mysql-preview capture-full --config /private/capture.json --out /private/run/capture
python3 tools/mysql-preview/prepare-native.py \
  --capture /private/run/capture --out /private/run/inputs \
  --secret-ref secret://warehouse/mysql-reader
oo-source-admission seal-native \
  --report /private/run/inputs/depots/report.json \
  --config /private/run/inputs/depots/config.json --out /private/run/sealed
```

Capture emits `table-0.arrow`, `schema-preimage.json`, `type-preimage.json`, `driver.json`, `access.json`, `reader-write-audit.json`, `connection.json`, and finally `capturer.json` (`oo.mysql.full-capture/v1`). The existing preparer emits `oo.source-admission.capture-witness/v1`, report, and config, carrying actual observed privileges and the table consistency grade (S0 for the existing single snapshot, B for multiple snapshots). The existing sealer emits its receipt, registry, and catalog, binding actual Arrow/schema/type/driver/audit bytes. The capturer is not a self-issued admission receipt.

For existing native worker integration, use `native-producer.py --config /private/producer.json` with:

```json
{
  "capture_config": "/private/capture.json",
  "driver": {"path": "/opt/oo/mysql-preview", "sha256": "MEASURED_DRIVER_SHA256"},
  "admission": {"path": "/opt/oo/oo-source-admission", "sha256": "MEASURED_ADMISSION_SHA256"},
  "state_dir": "/private/capture-state",
  "table": "depots",
  "prepare": "/opt/oo/tools/mysql-preview/prepare-native.py",
  "secret_ref": "secret://warehouse/mysql-reader",
  "registry_template": "/private/registrations/{binding}/{pin}/registry.json",
  "target": {
    "host": "postgres.internal",
    "port": 5432,
    "database": "warehouse_shadow",
    "runtime_principal": "oo_loader",
    "password_file": "/run/secrets/pg-loader",
    "ssl": "verify-full",
    "ca_file": "/run/secrets/pg-ca.pem"
  }
}
```

stdin is the existing worker occurrence:

```json
{"protocol":"oo.jobs.native-producer/v1","source_id":"warehouse","method":"C5","binding":"00000000-0000-4000-8000-000000000002","pin":1,"acknowledged_source_generation":null}
```

stdout is one `oo.pipeline.native-update/v1` JSON document with exactly the existing downstream fields:

- `source_engine:"mysql"`, `source_id`, `source_generation`, `parent_source_generation`;
- `logical_schema`, `logical_table`, `batch_id` (`binding_pin`), `run_id`;
- `capture_method:"c5_diff"`, `full_snapshot:true`, `delete_detection:"reconcile"`, `consistency_grade:"S0"`, `cross_table_consistent:false`;
- `position_codec:"snapshot-zero/v1"`, `position_lineage:"<source_id>.<table>"`, `applied_pos:"0"` (protocol sentinel, not a source commit position);
- `snapshot:{kind:"mysql_consistent_snapshot",value:"<run UUID>"}`;
- `columns` (sealed fields plus `logical_name`), `key_columns`, `tombstones:[]`;
- `arrow_path`, `max_arrow_bytes`, `admission_path`, configured `target`, `mirror:true`;
- `config_digest`, `schema_digest`, `type_digest`, `input_digest`, and `audit_digest` from the sealed binding.

Complete tables are S0, not a source commit T; separate per-table snapshots have cross-table B semantics. Loader still requires the existing operator registration and reviewed admission binary pin. The worker alone imports and ACKs. Changing the validator requires measuring/pinning the reviewed deployment artifact and rebuilding the Loader; this tool cannot bypass that trust boundary.

## Anonymous live verification

After integrating the reader dependency, build with `CARGO_BUILD_JOBS=2 cargo build --locked --manifest-path tools/mysql-preview/Cargo.toml --bins --examples` and similarly build source-admission. Run:

```sh
python3 testing/mysql-full-c5/verify.py \
  --driver tools/mysql-preview/target/debug/mysql-preview \
  --admission tools/source-admission/target/debug/oo-source-admission \
  --checker tools/mysql-preview/target/debug/examples/check_full_fixture \
  --out /private/NEW-fixture-evidence
```

The verifier covers both same-principal/separate-connection and separate-principal observers, verifies exact thread/time-window coverage against source general-log records, and retains a private mounted `reader.cnf` for the existing full-only fixture mysql-client route. Only a fresh `oo-yjcap-full-*` container is created. Credentials are random/private, not committed. The verifier independently compares root SQL values against actual Arrow IPC values, exercises actual capture/admission/request/replay, interrupts an in-flight source read and restarts the table, and rejects budget, deadline, approval, and unavailable-audit failures. It leaves its named fixture for inspection and never stops existing containers. Fixture target is empty intentionally: this verifies source bytes and admission, **not target import or publication**.

## Independent source-SQL transport handoff

The network `mysql-preview source-sql` path keeps `oo.mysql.source-sql/v1`, the existing single-SELECT validator, private secret files, read-only transactions, statement/row/byte limits, and mysql-client-compatible output. TLS remains required by default: `ca_file` must be an absolute CA path, with an optional verified `tls_server_name`. Explicit `tls:"required", plaintext_authorized:false` is also accepted. A dummy CA or a failed TLS handshake never triggers plaintext fallback.

For a trusted operator's no-TLS full/C5 route only, source-SQL accepts this explicit connection policy alongside its existing endpoint/user/password-file fields:

```json
{"tls":"disabled","plaintext_authorized":true,"ca_file":null,"tls_server_name":null}
```

`tls_server_name` may be absent in standalone requests; `ca_file:null` and the two policy fields must be present. Unknown, incomplete, or conflicting policies refuse. The standalone CLI is an operator tool, not a browser SQL API. Before any requested SELECT, this branch checks actual supported MySQL version (not MariaDB), compatible session mode, current user, read-only grants, a valid binary logging flag (`0` or `1`), `have_ssl=DISABLED`, and an empty cipher. It then uses the same read-only transaction and budgets as verified TLS. On failure/deadline, bounded same-user cleanup kills only the actual still-owned connection ID using the selected transport, never a TLS reconnect for a plaintext connection. Plaintext is **not encrypted** and produces no TLS proof. Binary logging does not authorize a transport downgrade or turn this metadata/query helper into a C1 reader.

For network source-onboarding, retain `onboarding.product.source_access:"network"`, the pinned `onboarding.producer`, `onboarding.container`, installed endpoint/SecretRef binding, and secret-root mapping. Only the explicit installed operator block below enables the legacy request; root `ca_file` and `tls_server_name` must both be explicitly null:

```json
{"ca_file":null,"tls_server_name":null,"onboarding":{"full":{"source_transport":{"tls":"disabled","plaintext_authorized":true}}}}
```

This fragment is not a complete operator configuration. Do not add transport fields to the public six-field input. Without the installed full policy, network requests still require a CA; normal C1 does not forward legacy policy. Profile-account selection and private secret-root checks remain unchanged.

The existing isolated fixture route also works without a new API: installed `onboarding.full` must be an object, with `onboarding.product.source_container:"oo-yjcap-full-<exact fixture suffix>"` and `onboarding.product.source_client_file:"/run/secrets/reader.cnf"`. The verifier creates that private mounted defaults file and checks the same mysql-client output shape used by source-onboarding. C1 still rejects this fixture prefix; arbitrary customer containers are not admitted by it.

Frozen/bounded independent source-cell comparison remains mandatory. This transport extension supplies neither source audit records nor customer acceptance: existing general-log/audit evidence must still come from actual source records, never client statement notes. Capture/admission audit requirements and the coordinator-owned customer audit block are unchanged.

The focused regression command below requires Docker and the `mysql:5.7.44` image. It creates its own small isolated no-TLS fixture with private random credentials, checks mysql-client output parity, read-only/session/grant refusal, row/byte/statement limits, and exact same-user cancellation against source general-log records. It stops only its own container on success; private temporary credentials and the stopped fixture remain for inspection. It performs no customer operations.

```sh
CARGO_BUILD_JOBS=2 cargo test --locked --manifest-path tools/mysql-preview/Cargo.toml \
  --test source_sql_live explicit_plaintext_full_transport_on_an_isolated_mysql_fixture -- --exact
```

## Large-table downstream boundary

Chunk capture writes each part plus a compatible combined Arrow stream for v1
admission. It does not lift existing Loader per-import byte/row caps or add a
second Loader. A very large combined stream cannot be passed through the old
single-request native-producer import unchanged. Use the part files with a
reviewed multipart staging/import integration before claiming target publication.
Source-to-Arrow scale evidence is in `testing/mysql-full-c5/w1-a/`; no PostgreSQL
Loader/publication success is claimed by that evidence.
