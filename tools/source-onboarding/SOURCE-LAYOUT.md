# Whole-source planning and offline preflight

Run before provisioning or copying. These tools never open a database connection.

```sh
python3 tools/source-onboarding/source_layout.py preflight --input inventory.json --out preflight
python3 tools/source-onboarding/source_layout.py plan --input inventory.json --out layout
python3 -m unittest discover -s tools/source-onboarding -p test_source_layout.py
```

The output directory must not exist. Preflight exits 2 for blockers; output remains
available for review. `importable` is metadata-only, not admission or publication
approval: values, source evidence, credentials, TLS approval and budgets still need
runtime validation. No raw data, addresses or credentials belong in this export.

Input is a normalized information_schema JSON export plus separately collected
exact counts and session facts (not raw vendor column names):

```json
{
  "session": {"sql_mode":"STRICT_TRANS_TABLES,NO_ZERO_DATE", "time_zone":"+00:00", "have_ssl":"YES", "log_bin":"OFF", "privileges":["SELECT","LOCK TABLES","SHOW VIEW"]},
  "tables": [{"schema":"sample", "table":"t_items", "domain":"catalog", "modeled":true,
    "estimated_rows":0, "exact_rows":1200,
    "primary_key":[{"name":"id", "type":"bigint unsigned"}],
    "columns":[{"name":"id", "type":"bigint unsigned"}]}]
}
```

`unique_key` can substitute for a missing primary key; supply only an already
validated, wholly non-null, non-prefix unique index in index order. A `prefix`
hint substitutes for `domain`; otherwise the first name component is used. Review
the inferred domains: lexical prefixes do not establish business relationships.
Every row count must be exact; missing counts fail closed. Empty keyless tables
are excluded with a recount upgrade condition, never silently omitted.

`sources.json` is a list of `{key, source_id, shard, tables, rows, schema, domain,
publish}`. In the standalone source-only workflow, modeled groups split at 16
and unpublished groups at 32; its project uses one entire modeled source. The
library workflow below combines shard sources under one publication/role/Pack,
without bypassing the independent compiler budgets. UUIDs are deterministic for a membership set; changing membership changes
identity and requires controlled retirement of old current-table ownership.

Each `sNNN-manifest.json` uses the existing per-table `chunking` and `budgets`
shape. It is a planning artifact, not a runnable driver config: bind private
connection settings, artifact pins, policies and publication target through the
existing onboarding recipe. Composite keys (including empty tables), large tables,
and unreliable estimates select chunks if a usable key exists. `max_rows` is the
per-chunk budget, not the whole-table count. Keyless tables remain bounded snapshots;
large keyless/LOB tables may still be refused at runtime. No tolerance or lossy
normalization is silently enabled. See [field notes](../../docs/database-support/06-real-source-field-notes.md).

## Multi-source execution

`source_scheduler.py --config run.json --out run-001` executes existing commands
(no shell). `--retry-failed run-001/summary.json` selects only failed/unfinished
tables; use a new output directory. Provision commands must exit only when the
source is ready; this runner waits for exit 0 before table commands. Each source
runs tables serially, with bounded concurrent sources and staggered provisioning.
Use the existing `provision.py` and onboarding recipe argv, not a second Loader.

```json
{"concurrency":2,"stagger_seconds":10,"provision_timeout":600,"table_timeout":86400,
 "cost_guard":{"metrics_file":"/private/run/metrics.json","max_egress_bytes":20000000000,"min_balance":10,"max_age_seconds":60},
 "sources":[{"key":"s001","provision":["/approved/provision-wrapper","--wait-ready"],
 "tables":[{"table":"t_items","argv":["/approved/onboard-wrapper","--table","t_items"]}]}]}
```

Optional `tunnel` has `argv`, `probe` (exit 0 means ready), `max_attempts` (5),
`backoff_seconds` (5), `ready_timeout` (30). Supply an approved foreground SSH
command with `ExitOnForwardFailure=yes`, `ServerAliveInterval=15`,
`ServerAliveCountMax=3`; never inline credentials or disable host-key checking.
Only owned process groups are stopped. Provision/capture launches wait for probe
success. A lost tunnel is restarted with bounded backoff; in-flight failed tables
are recorded, not silently retried or declared successful. Provision wrappers must
be idempotent and preserve existing ownership. Detached remote jobs need their
existing cancellation/cleanup protocol; killing a local client cannot prove remote
cancellation. Do not use detached wrappers without that protocol.

The cost metrics producer is deployment-specific and **not** supplied here. Write
atomically `{ "observed_at": <Unix seconds>, "egress_bytes": <cumulative billed
outbound bytes for this run>, "balance": <remaining currency> }`. Threshold units
must match the producer; include both capture and independent reconciliation.
Missing, malformed, stale or future metrics pause fail-closed. Reaching either
threshold stops owned work, records unfinished tables and exits 75; restore budget
and explicitly resume failed work. There is no automatic top-up or automatic
publication. Without `cost_guard` no cost protection is claimed. Logs/summary are
private runtime artifacts, not repository fixtures. Re-running provisioning before
a failed-only retry is deliberate. Review artifact pins before every retry.

## Library entry point (layout and import)

A library identifies one logical database; shard sources are transport details, not
separate Agent endpoints. `library.py` is the product entry point for offline
layout and **initial** import, wrapping the same planner and scheduler above:

```sh
python3 tools/source-onboarding/library.py plan --library-id sample-library \
  --input inventory.json --out library-layout --split domain \
  --table-cap 16 --concurrency 2 --stagger-seconds 10
python3 tools/source-onboarding/library.py import \
  --manifest library-layout/library.json --binding private-run.json --out import-001
python3 tools/source-onboarding/library.py import \
  --manifest library-layout/library.json --binding private-run.json --out import-002 \
  --retry-failed import-001/library-result.json
python3 tools/source-onboarding/library.py status \
  --manifest library-layout/library.json --result import-002/library-result.json
```

`oo.library.manifest/v1` contains `library_id`, the complete `inventory`
(schema/table identities), `sources`, per-shard `manifests`, `exclusions` (reason
and upgrade condition), `layout`, and `scheduling`. Every inventory entry must
belong to exactly one shard or exclusion. Duplicate sources, tables and uncovered
inventory entries are rejected before execution. Source UUIDs include the library
identity and deterministic membership; re-planning membership does not retire an
existing source. Do not re-plan an already imported database to adopt it.

Configuration:

| Option | Default | Accepted values / meaning |
|---|---|---|
| `--split` | `prefix` | `prefix`: explicit prefix or first name component; `domain`: domain then prefix fallback; `explicit`: every non-excluded table needs `domain`; `size`: deterministic greedy balance by exact row count within each schema |
| `--table-cap` | 16 | 1–4096, maximum tables per shard; planning does not lift runtime publication or Pack limits |
| `--concurrency` | 2 | 1–32 concurrent sources |
| `--stagger-seconds` | 10 | Finite nonnegative provisioning-start interval |
| inventory `exclude_reason` | absent | Explicit nonempty reason; optional `upgrade_condition`, default explicit inventory review |

Size splitting balances **rows**, not measured bytes. Exact counts remain required
for imported tables. Empty keyless exclusions retain the planner's recount rule.
The existing source-only planner defaults and limits are unchanged.

The private binding has the scheduler's `sources` argv shape plus optional
`cost_guard`, `tunnel`, `provision_timeout`, `table_timeout`. It must cover exactly
the manifest's source keys and tables; concurrency and staggering come only from
the manifest. Bind argv through existing provisioning/onboarding recipes with
reviewed pins and credentials out of repository. Commands are run without a shell.
Cost protection is optional and is never claimed when not configured.

Import writes the unchanged scheduler `summary.json` and a cumulative
`library-result.json` (`oo.library.import-result/v1`). Retries select failed and
unfinished identities only and retain earlier successful results; a changed
manifest digest or library id rejects resume. `status` reports expected, imported,
excluded, failed and pending counts; `complete` means inventory reconciliation,
**not publication readiness**. `published` is always false and publication is
`unverified`: the import subcommand does not publish; use the explicit publish/adopt steps below. Successful argv completion is import-runner evidence, not an
independent SQL correctness oracle. Status exits 1 for incomplete coverage; import
exits 75 for a guard pause, 1 for failed coverage, 2 for invalid inputs.

Limits and upgrade conditions: startup errors and catchable interruptions preserve
a cumulative result; SIGKILL/power loss may leave only a scheduler checkpoint.
Import completion does not bypass publication or Pack gates.

## Library publication and adoption without re-import

`publish` (after initial import) and `adopt` (existing sources, including already
published shards) intentionally use the same path. Neither imports data. The
planner manifest is accepted directly; for existing shards use the closed
publication membership format, preserving source IDs:

```json
{"protocol":"oo.library.membership/v1","library_id":"sample-library","sources":[
 {"source_id":"existing_source_a","schema":"sample","tables":["t_items"]},
 {"source_id":"existing_source_b","schema":"sample","tables":["t_events"]}
]}
```

Every member's **entire current table set** must be listed. Membership is fixed
once registered and a source cannot belong to two libraries. Keep the full
inventory and reasoned exclusions separately (or pass the full planner manifest);
membership alone does not describe excluded/pending database objects.

The private publication binding (not the import scheduler binding) is:

```json
{"pipeline":{"path":"/approved/oo-pipeline","sha256":"<64 lowercase hex>"},
 "coordinator":{"path":"/approved/release-coordinator","sha256":"<64 lowercase hex>"},
 "target":{"host":"agentdb.example.invalid","port":5432,"database":"agentdb",
   "runtime_principal":"oo_target_admin","password_file":"/private/target-password",
   "ssl":"verify-full","ca_file":"/private/target-ca.pem"},
 "release_config":"/private/release-maintain.json",
 "activation":{"project_id":"sample","candidate_id":"library-candidate",
   "source_hash":"<staged candidate source hash>","expected_version":null,
   "schema_name":"sample","relation_name":"t_items","role_name":"oo_r_library"}}
```

Pin real binaries first. The target connection uses an administrator authorized to
read current metadata and invoke library publication, not a source credential.
The existing coordinator maintenance config supplies control/target admin and
readonly reader file references and installed deployment_mode. A `replica_pg`
endpoint is required for actual primary/replica readiness; all-in-one forbids it.
The reader needs CONNECT, USAGE `_oo`, mapped lookup EXECUTE, SELECT on
physical_mappings/full_import_run/full_import_table; data privileges stay on the
single NOLOGIN data role. Existing target-admin ownership/GRANT and role-session
termination capabilities are required. Do not grant owner membership to readers.

The reviewed whole-library Pack must already be staged through the existing
release API/coordinator. These commands do not approve ontology, waive gates or
create a production-ready certificate. The compiler's independent Pack limits
still apply; large-library Pack budget changes belong to the Builder integration.

```sh
python3 tools/source-onboarding/library.py adopt --manifest membership.json \
  --binding private-publication.json --out adopt-precheck --prepare-only
python3 tools/source-onboarding/library.py adopt --manifest membership.json \
  --binding private-publication.json --out adopt-001
python3 tools/source-onboarding/library.py publish --manifest layout/library.json \
  --binding private-publication.json --out publish-001
python3 tools/source-onboarding/library.py status --manifest membership.json \
  --binding private-publication.json
python3 tools/source-onboarding/library.py adopt --manifest membership.json \
  --binding private-publication.json --out recovery-001 --recover
```

Output directories must be new. Precheck invokes `oo-pipeline native-library-prepare`
against current target metadata, reuses existing source receipts, and calls the
real SQL publication function in a transaction it **rolls back**. No library
registration or publication survives prepare-only. Runtime writes precheck.json,
activation.json, release-receipt.json; preserve these private metadata artifacts.
Actual activation uses existing coordinator gates and same-transaction all-table
fence opening. A successful result requires knowledge_published and the exact
library receipt digest in the release result, not only child exit 0.

The SQL protocol, canonical digest, default ceilings (64 shards/4096 tables),
superseded state, immutable receipts, pending/ready/closed pointer and replica
barriers are specified in [ADR-029](../../docs/adr/ADR-029-library-layout-publication-and-adoption.md).
Single-source publication stays unchanged, but the coordinator refuses independent
source releases after adoption. Old source receipts remain evidence, not authority.

`status --binding` rechecks the real current vector without committing publication,
returns generation_vector_verified and publication (unpublished/pending/ready/closed).
`--result` remains runner-only and cannot be combined with `--binding`. A complete
inventory is not the same thing as ready. Post-commit visibility failure closes
all library fences and the head, preserving committed receipts. `--recover` only
reopens the same active candidate and same immutable binding; changed data vectors
need a new reviewed/staged candidate, never revival of an old generation.
