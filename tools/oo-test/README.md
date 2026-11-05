# oo-test: owned synthetic fixtures

Python 3 standard library + Docker CLI. No root Cargo dependency/lock changes.
Run from this checkout; the tool resolves paths relative to its own file.

```sh
tools/oo-test/oo-test matrix --engine mysql --versions 5.7,8.0,8.4,9.x --suites T0
tools/oo-test/oo-test matrix --engine postgres --versions 13,14,15,16,17,18 --suites T0
tools/oo-test/oo-test matrix --engine oracle --versions 23ai,21c,11g --suites T0
PYTHONDONTWRITEBYTECODE=1 python3 -m unittest discover -s tools/oo-test -v
```

Default seed: 2,000,000 events, 200 small tables, 300-column wide table.
`--scale 1000 --tables 3` is a **reduced smoke test**, not scale acceptance.
Versions resolve only to matching **locked v0.2** patches, never a registry tag
or a legacy SP00 pin. T1 and T2 share this resolver.
PostgreSQL 12 is selectable but experimental. Oracle `23ai` resolves to 23.5,
not the later 26ai releases that also start with 23.

The runner executes one source database at a time, creates
an explicitly labelled data volume, and uses platform manifest digests.
Fixture-only runs publish no host ports. Runs with product hooks reuse the
standalone runner's ephemeral loopback ports instead of the Docker exec relay;
hook teardown must not terminate source backend processes.
Missing native architecture selects `linux/amd64`; resource receipts say
`emulated` when this differs from the Docker server architecture.
`--platform linux/amd64` overrides selection. Native readiness timeout is
300 seconds, Oracle/emulated timeout 1200 seconds (`--health-timeout` override).
Registry pulls retry at most three times; each failed attempt remains evidence.
`--runner remote` fails explicitly without attempting SSH.

## Evidence and acceptance boundaries

Each invocation prints a timestamped `evidence/h-s5-<timestamp>-<uuid>/`.
Historical S0 runs retain their original names. Each version directory contains:

- Numbered argv/exit records and stdout/stderr (passwords and signed URL query
  strings redacted; SQL containing passwords travels through stdin only).
- `seed.json`, `fixture-shapes.json`, `gold-answers.json`.
- `comparison-report.json`, `resource-receipt.json`, `SHA256SUMS`.
- A private temporary directory, deleted during cleanup; never commit it.

Use `--baseline evidence/<other-run>/<engine-version>/gold-answers.json` to
compare all 20 source-computed answers across engines. Without a baseline,
the first successful version is **reference-only**, not independent correctness
proof. Subsequent versions must match; mismatch is a nonzero exit. Preserve
failed runs and use a fresh run id for retries.

T0 here is the card's fixture precheck: boot, admin seed, separate read-only
query principal, table counts, 300-column check, exact version and gold.
It is **not** product `oo doctor`, permission diagnostics, T1-T8, an Arrow
round-trip, or product acceptance. Consistency is explicitly `B` (quiescent
owned fixture; no claim of transactional snapshot across separate clients).
Final Docker stats are a sample, not measured peak/duty-cycle proof; unknown
metrics are null. A nonzero command or unimplemented requested suite fails.

Cleanup checks both `oo.run_id` and `oo.lane=h` before removing named resources.
No prune, image deletion, anonymous volume deletion or unrelated-container
operations. SIGINT/SIGTERM attempt cleanup; SIGKILL/host failure cannot, so use
the receipt's exact names/labels for manual recovery. Pulled images stay cached.

## Engine-lane hooks

```sh
tools/oo-test/oo-test matrix --engine postgres --versions 17 \
  --suites T0-T2 --hook T1=./my-t1 --hook T2=./my-t2
```

Hooks are executables, invoked without shell evaluation after seeding. They
must return nonzero on any failure and write explicit `index-rows.json` under
their evidence directory. A zero exit alone fails closed. `OO_TEST_CONTEXT` points to a JSON file
with `container`, `engine`, `version`, `run_id`, `evidence`, `reader`, and
`reader_password_file`. The container has no exposed host port. Use `docker
exec` or a lane-owned client on its network. Do not put passwords in argv.
The MySQL reader defaults file inside the container is `/run/oo/reader.cnf`;
PostgreSQL reader uses the local socket; Oracle reader uses the password file
via SQL*Plus stdin and the FREEPDB1/XEPDB1 service (11g uses XE).
Only the separate fixture admin may apply growth/change workloads. Product
connections must always use the read-only reader. Hooks own their additional
labelled resources and evidence; a zero hook exit is not automatic capability
admission. Missing product hooks are `not_implemented`, never skipped-as-passed.
T7 is built in and cannot be replaced by a hook.

## Source audit (T7)

Requesting T7 automatically includes T1 and T2. The audit is enabled after
admin seeding and before either hook. It collects through all requested hooks,
even after a hook returns nonzero. A failed dependency, empty audit, missing
boundary marker, hook without observed reader activity, malformed/truncated
record, or unrecognized statement fails T7. Raw source output and the parsed
`audit-report.json` stay in the evidence directory.

- MySQL: general log to TABLE, started as `mysqld --general-log=1
  --log-output=TABLE` so fixture restarts (TLS, binlog) cannot reset it; only
  reader rows (and their `Connect` rows) enter the proof.
- PostgreSQL: CSV collector (UTC), fixture-global `log_statement=all`,
  `log_replication_commands`, connection/disconnection logging and error
  statement logging (`ALTER SYSTEM`, so restarts keep it), filtered to the
  authenticated reader. Role startup GUCs are not changed (the product rejects
  them). Logged bind parameters are kept for slot-feedback attribution.
- Oracle 23ai: reader-only unified audit policy, plus a CDB$ROOT policy on the
  `C##OO_MINER` account for opt-in C1. Oracle XE 11g/21c in mixed mode:
  standard `DB,EXTENDED` auditing configured before seed via an owned restart,
  read from `SYS.AUD$` (full CLOB SQL; `DBA_AUDIT_TRAIL` truncates). Rows are
  exported as JSON CLOBs; truncated SQL is a rejection.

`audit.classify` assigns every record exactly one reviewed category or
rejects it with the named rule miss (`write_keyword:*`,
`unreviewed_function:*`, `unreviewed_setting:*`, `unreviewed_optimizer_hint`,
`replication_command_outside_c1`, `write:<ACTION>:<OBJECT>`, ...):

| Category | Admits |
|---|---|
| `read`, `catalog_read`, `c5_digest`, `logminer_read` | SELECT/WITH with only reviewed side-effect-free functions; no INTO, row locks, sequences or data-modifying CTEs |
| `read_cursor`, `read_explain`, `read_show` | PG DECLARE...FOR <read>/FETCH/CLOSE; EXPLAIN without ANALYZE; SHOW/DESCRIBE |
| `txn_control` | BEGIN/START TRANSACTION (never READ WRITE), SET TRANSACTION isolation/READ ONLY/SNAPSHOT, COMMIT/ROLLBACK, `pg_export_snapshot()` |
| `d12_session_state` | reviewed session settings only (`SET`, `set_config(name,..,false)`, MySQL user variables, `ALTER SESSION SET NLS_*/TIME_ZONE/...`, DBMS_APPLICATION_INFO/DBMS_OUTPUT) |
| `replication_protocol` (C1 windows only) | MySQL Binlog Dump/Register Slave; PG walsender reads; `pg_replication_slot_advance($1,$2)` progress feedback when `$1` is a pre-existing fixture slot |
| `logminer_call`, `d11a_*` (Oracle C1 miner only) | DBMS_LOGMNR ADD_LOGFILE/START_LOGMNR(online catalog)/END_LOGMNR; Oracle-internal DML on SYS/SYSTEM `LOGMNR*` objects and the following COMMIT |
| `d12_logon_bookkeeping`, `d12_dictionary_bookkeeping`, `d12_audit_trail` | exact SYS/AUDSYS (object, action) pairs, only with a privilege proof that reader accounts hold no DML/DDL privileges |

Any rejected record anywhere in a fixture, including harness reads outside
product windows, fails every variant of that fixture.

### Product T7 driver

`t7.py` runs the existing T1 (`t1.run_t1`) and T2 (`t2.run_postgres`,
`t2_p0.run_cycles`) workloads on owned `oo.lane=t` fixtures with the audit
enabled before the reader exists, records each product invocation as a
window (database clock before/after; variant from `--c5`, `--sync`,
`--handoff` or the source id), and writes `t7/<engine>-<version>/t7-<kind>.json`,
`t7-<kind>-records.json` (every record with category and rule) and explicit
`T7` `snapshot`/`c1`/`c5` rows via `verdicts.write_rows`. A variant passes only
if its workload completed, its windows hold audited product statements and
the fixture has no rejected record. Oracle XE C1 rows are `skip` (not run).

```sh
python3 tools/oo-test/t7.py --engine mysql --versions 5.7,8.0,8.4,9 --run-id t7-run
python3 tools/oo-test/t7.py --engine oracle --versions 23,21,11 --run-id t7-run \
  --worker-image <thick-worker-image>
tools/oo-test/oo-test matrix --merge-only --merge-evidence evidence/t7-run/t7
```

Only `evidence/<run>/t7` is merged; T1/T2 rows written by the wrapped
workloads under `workloads/` use reduced fixtures and are not admission
evidence.

```sh
tools/oo-test/oo-test matrix --engine postgres --versions 17 \
  --suites T0-T2,T7 --hook T1=./my-t1 --hook T2=./my-t2
PYTHONDONTWRITEBYTECODE=1 python3 tools/oo-test/audit_smoke.py
```

`audit_smoke.py --targets mysql:8.4,postgres:17,oracle:23ai` boots reduced
owned fixtures and checks both read capture and a denied DELETE. It does
**not** run product snapshot/incremental workloads or produce T7 admission
evidence. Raw failed attempts are retained alongside successful retries.

## Nightly matrix and evidence index

```sh
tools/oo-test/oo-test matrix --all --suites T0-T4,T7,T8
```

`--all` covers all 14 P0 version targets, including a failed Oracle 19c entry
when its licensed image is not locked. It does not silently exclude that
target or claim P1/P2 coverage. Execution remains serial with automatic
amd64 selection for images without native support. Missing product hooks
fail the target before pulling/starting a container; every requested suite
still receives an evidence row. Interrupted targets also receive failure
rows. No unrelated container, image or volume is removed.

`evidence-index.json` is a list compatible with
`specs/scripts/gen_support_matrix.py`. Rows contain `target` (support-matrix
ID), `version`, `capability`, `suite`, `variant`, `result` (`pass|fail|skip`),
`run_id`, `sha256`, `evidence`, `emulated`, `version_measured`, UTC `observed_at`
and error detail. Each runner writes `index-rows.json`; the hash binds its raw
JSON result, and `SHA256SUMS` additionally binds command outputs. The matrix
checks raw hashes before merging rows. Pass rows require a measured version.
Unstarted targets use the locked image version with
`version_measured=false`, never a claimed observed server version.
In particular, an Oracle image tag is not an observed five-component DB version.

Standalone T1/T8, PostgreSQL and MySQL/Oracle T2/T8, and T3/T4 runners emit the
same contract. T3/T4 subset runs cannot pass the whole suite; Oracle 11g/21c
C1 fault paths are skipped, not promoted. Failed cleanup invalidates admission.
T8 rows require explicit meter verdicts, not just a successful loader exit.
Use `--merge-evidence evidence/<run-id>` (repeatable) to ingest standalone runs
alongside the matrix run. It only accepts directories inside this worktree's
ignored evidence tree. Example:

```sh
OO_TEST_LANE=h tools/oo-test/oo-test matrix --all --suites T0-T4,T7,T8 \
  --merge-evidence evidence/<standalone-run-id>
```

This merges observations; it does not supply missing product hooks, turn a
fixture precheck into `oo doctor`, or make separate fixtures one audited T7
session. Missing hooks remain failures. T5/T6 are never implied by these rows.
Every matrix target and standalone source setup checks at least 20 GiB free
before starting containers, waiting up to 30 minutes for space to recover;
disk blocks are recorded as non-passes. Cached pinned images are reused without
pulling; a missing image requires at least 25 GiB free before a pull. There is no
automatic deletion of images, volumes, Cargo artifacts or unrelated data.

For a consolidated run assembled from sequential standalone suites, use
`oo-test matrix --merge-only --merge-evidence evidence/<run-id>` (repeat the
evidence option for every run). This validates raw hashes without starting
new targets or replacing observations with a fresh missing-hook failure.
It preserves every explicit verdict, including failures, and creates no
verdict for a suite that did not run.

Use `--variant c1` or `--variant c5` to identify the hook path, also exposed
in `OO_TEST_CONTEXT`. Run each path separately; a generic incremental hook
does not establish either variant. Missing T2/T3/T7 evidence emits failure
rows for both variants to prevent an old CDC/digest pass surviving omission.
Bare fixture T0 exports `skip`, not a product-doctor pass; install a real
`--hook T0=PATH` to produce product T0 evidence.

### Product T0 hook

```sh
cargo build --locked -p oo-cli
CARGO_TARGET_DIR="$PWD/target" cargo build --locked --manifest-path deploy/lite/mysql-doctor/Cargo.toml
OO_TEST_LANE=h tools/oo-test/oo-test matrix --all --suites T0 --hook T0=tools/oo-test/t0.py
```

For Oracle 11g, also set `OO_TEST_ORACLE_WORKER_IMAGE` to the content ID of
the trusted, locally built thick-worker image. Oracle 19c remains an explicit
failure until its licensed image is locked.

The hook runs the real `oo doctor --json` with an isolated temporary HOME
and generated fixture-only secrets. It checks the measured version, capability
states, missing-privilege DBA advice, chunk plans/unknown byte estimates and
the agreement of JSON readiness with process status in `auto`, `C5` and `C1`.
Planning covers `events` and `order_lines` on the seeded fixture, not all
tables. The reader has snapshot grants but lacks C1 setup: a correctly
diagnosed C1 block passes this diagnostic case, **not C1 acceptance**.
The model endpoint is a loopback synthetic replay; no paid calls or user
secrets are used. Admin-only TLS and Oracle FLASHBACK setup precede probing.
This hook can be combined with T1 on MySQL: both configure the owned host
reader idempotently, retaining SSL and read-only grants.

Doctor currently hardcodes lane A for its Oracle worker. A private exact-argv
Docker shim changes only that invocation's resource labels/name to lane H
and this run ID, forwards IO unchanged, and rejects all other invocations.
The hook checks ownership before cleaning up a stranded worker. This is test
resource isolation, not a replacement doctor or a changed capability verdict.
Raw JSON/exit records and hash-bound `index-rows.json` remain under
`product-t0/`; malformed reports, wrong versions, missing fields, readiness
disagreement, planning errors and cleanup failures cannot pass.

`downgrade-report.json` lists all non-passes and never edits shared matrix
files. The coordinator must ingest the index and apply its generator;
capability promotion still requires review of the hooked tests. A zero
exit from an arbitrary hook is not independent proof of its correctness.
The local/CI launcher and three-attempt stability reporting are described
in `ci/README.md`. No scheduler is installed by these tools.

## Comparison API

Import `comparison.py` by adding `tools/oo-test` to your Python module path.

```python
from decimal import Decimal
from comparison import compare

report = compare(
    [(1, Decimal("2.0000")), (2, None)],
    [(1, Decimal("2")), (2, None)],
    key_columns=[0], chunk_size=10000, consistency="S1",
)
assert report["equal"]
```

`compare(left, right, *, key_columns, chunk_size=10000, consistency="B",
oracle_empty=False)` consumes two iterables once, bounded to one row per side.
The caller MUST supply rows already ordered **by the source's selected key
and collation**, from the appropriate snapshot. Never Python-sort varchar
keys. Composite keys use multiple indexes. No-key callers must supply a
stable source-defined identity/order or decline comparison.

Output includes ordered per-chunk row counts and SHA-256 hashes, the first
differing row offset and both keys (one null for a missing tail), the chosen
consistency level, and explicit Oracle empty-string policy. Missing/extra rows,
changed keys, order changes and value differences fail equality.

`canonical(value, oracle_empty=False)` supports Python int/Decimal (lossless
decimal text, insignificant zeroes removed), float (IEEE-754 binary64 bits,
including signed zero/NaN payload), str, bool, None, bytes, date, datetime,
lists and string-keyed dicts. Aware timestamps retain UTC microseconds plus
original offset and IANA zone when available; naive timestamps are distinctly
tagged local timestamps, never silently assigned UTC. Pass Decimal, not
float, for exact database numerics. Text is not case/accent/space-normalized:
key collation controls ordering, not stored-value equality. Decode JSON into
dict/list for semantic object-key normalization; use str for textual equality.
`oracle_empty=True` collapses SQL-cell empty text to NULL explicitly, never
empty strings nested inside JSON objects/arrays; source provenance must be
retained by the calling lane.

The hash format is `oo-test-typed-json-v1`: UTF-8 canonical typed JSON per row,
compact separators, sorted object keys, plus one LF. It is intentionally
**not** `ChunkReceipt.sha256` (Arrow IPC) or `RangeDigest.digest`
(order-independent C5). Do not compare these hash domains directly.
`TableRef`, watermarks and plan revision remain engine-owned metadata; attach
this oracle's report to them without changing their serialization.
## Lane M incremental T2

```sh
tools/oo-test/oo-test t2 --versions 13,14,15,16,17,18 --run-id pg-t2-local
```

This entry owns one labelled PostgreSQL container at a time, with distinct
source/Agent databases and separate reader/admin connections. It runs the real
`oo-load` binary against a compact two-table fixture: three C1 and three C5
cycles, concurrent admin writes, exact row comparison after each cycle,
monotonic durable C1 positions, T8 sampling and a no-change cycle with zero
logical target writes/new generations and unchanged total target relation size.
C5 additionally requires zero delivered payload on its no-change cycle.

It retains concurrent-range retry failures and requires a successful repaired
cycle; it does not mask unexpected source failures. Negative checks recreate a
replication slot and a source table, requiring explicit rebaseline errors.

Raw per-version commands, receipts, comparison hashes, resource/image receipts,
T8 samples, failures and cleanup results live under `evidence/<run-id>/`.
The command exits nonzero for missing/failed T2 or cleanup failures.

This does not exercise C5-to-C1 handoff, full type/scale fixtures or T7. MySQL
and Oracle T2 are not implemented by this entry; do not infer those engine
exits from the PostgreSQL results or the separate synthetic-source target tests.

## Lane N T3 fault injection and T4 schema change

```sh
tools/oo-test/oo-test t3t4 --engine mysql --versions 5.7,8.0,8.4,9 --run-id my-t3t4-mysql
tools/oo-test/oo-test t3t4 --engine postgres --versions 13,14,15,16,17,18 --run-id my-t3t4-pg
tools/oo-test/oo-test t3t4 --engine postgres --versions 18 --run-id x --cases slot_invalidation
```

Requires `target/debug/oo-load` built with `--features mysql-binlog`.
Images resolve only through the v0.2 lock (`runner.resolve`). Per version it
owns one source and one Agent DB container (`oo.lane=n`), both on fixed
loopback ports (the binding identity includes the source port) and the
Agent DB on a labelled volume so a restart keeps its data.

T3 (each: fault, then resume or a named refusal with the published target
unchanged, then zero divergence of columns and rows):

- `source_restart_mid_chunk`: `docker restart` of the source after >=3
  snapshot chunks committed; resume the same run ID.
- `network_drop_mid_c1`, `network_drop_mid_c5`: `docker network disconnect`
  for 20 s after the C1 position / C5 staging advanced.
- `target_restart_mid_publish`: Agent DB restart mid snapshot, mid C5
  replacement staging and mid C1 apply.
- `sigkill_mid_cycle`: SIGKILL of oo-load mid snapshot, C5 and C1.
- `retention_expiry`: MySQL binlog expiry (8.0+ `binlog_expire_logs_seconds`,
  5.7 `PURGE ... BEFORE NOW()`); PostgreSQL slot dropped by the DBA.
- `slot_invalidation` (PostgreSQL): `max_slot_wal_keep_size` WAL removal on
  13-18, plus `idle_replication_slot_timeout` on 18.
- `gtid_purge` (MySQL): GTID purge, and GTID/binlog reset until the durable
  file name and offset exist again with different history.

Refusals must be closed codes; a rebaseline is a new binding generation
(snapshot + handoff on MySQL, new slot + snapshot on PostgreSQL), verified
equal after further DML. Every C1 resume must keep durable positions
monotonic; snapshot resumes must not duplicate chunk receipts.

T4 (C5 and C1 bindings per case): add/drop/rename column, int->bigint,
add table, drop table, drop then re-add a (different) primary key. After the
DDL with no DML, and again after DML, each path must apply online with zero
divergence or refuse by name with the target unchanged; refused paths are
rebaselined and must reach zero divergence, except unsupported shapes
(keyless C1/PG C5), which must refuse by name.

Harness recoveries are recorded in `operator_recoveries`: after a
reconnect MySQL's host cache is cleared without binlogging, and a Docker
Desktop published port lost after reconnect is restored by a container
restart. Evidence is `evidence/<run-id>/<engine>-<version>/t3t4.json` plus
raw numbered commands. Compact fixture (bigint/int/varchar, 20,000-row bulk
table), consistency `B`; not full type/scale or T7 proof.

## Lane G gold dialogue (T6)

```sh
cargo build --locked -p oo-cli -p oo-builder --bins -p oo-gateway
CARGO_TARGET_DIR="$PWD/target" cargo build --locked --manifest-path tools/oo-load/Cargo.toml
CARGO_TARGET_DIR="$PWD/target" cargo build --locked --manifest-path tools/release-coordinator/Cargo.toml
tools/oo-test/oo-test t6 --engine postgres --run-id g-t6-pg --execute
tools/oo-test/oo-test t6 --engine mysql --run-id g-t6-mysql --execute
tools/oo-test/oo-test t6 --engine oracle --run-id g-t6-oracle --execute
tools/oo-test/oo-test t6 --engine postgres --version 13.23 --run-id g-t6-pg13 --execute
tools/oo-test/oo-test t6 --engine oracle --version 11.2 --worker-image LOCAL_THICK_IMAGE --run-id g-t6-ora11 --execute
```

Run engines serially: PostgreSQL uses one owned container; MySQL/Oracle use
one source plus one PostgreSQL Agent DB. All carry `oo.lane=g` and cleanup
checks ownership. Defaults remain PostgreSQL 17.11, MySQL 8.4.7 and Oracle
23.5; `--version` selects another locked source pin. PostgreSQL versions other
than 17 use a separate source and a PostgreSQL 17 Agent DB. Oracle 11g needs
an existing amd64 thick-worker image, started with lane/run ownership labels
and removed after the run. At most two database containers run concurrently.
`--scale 1000` is only a reduced diagnostic, not the default 2M-event run.

The harness extends `test-gateway-cycles.py`, seeds lane H's neutral fixture
with a separate admin connection, computes all 20 gold answers using a
read-only source account, and loads the 12 tables used by those questions.
Mapped schema/table/column identifiers and types come from the native
publication candidate, including Oracle's encoded uppercase identifiers.
Source names remain labels; the harness does not reimplement name encoding.
The default `--publisher reviewed` invokes the real retained Builder with a
call limit derived from its own retained-inventory preflight batch count plus
two repair calls per batch, writes an accept-all decision file, runs native
`oo review --review-file`, and calls `release-coordinator publish-retained`.
It does not open fences manually. Only these 12 gold tables are built, not
the complete fixture: this is **not** full-fixture T5 acceptance or a
privacy-review example for real data.

`--publisher fixture` preserves the first-run diagnostic path: deterministic
role Pack, synthetic pinned review, and directly opened test fences. Its
results are explicitly separate from the default product publisher path.

The real native `oo share-prompt` issues an enrollment code. A real-model
Agent receives that prompt and a constrained native CLI tool, binds using
`oo login` with stdin, verifies `oo whoami`, and answers each gold question.
Only login, whoami, tables, describe, agent-skill and query are exposed; no
shell, file-reading tool, source connection, gold SQL or gold answers are
available to the Agent. TLS paths, destination and token references are
operator-fixed. Enrollment codes and tokens are removed from evidence.

`--execute` authorizes paid calls. Before any call, stdout and
`preflight.json` show limits: 160 total Agent calls (including binding), 12 model
turns/question, 2,048 output tokens/call, 250,000 request bytes, 12M reserved
tokens (request bytes + output + 10k proxy allowance), one-hour dispatch
deadline, 300-second Agent egress timeout, serial execution and no retries.
Currency cost is unmeasured.
The real Builder's derived call bound and separate preflight/usage receipts
are printed before Builder dispatch, including the total bound across Builder
and Agent (160 + three times the planned batch count). Limits outside the
Builder's supported 1..4096 range fail before dispatch. Replanning can change
the actual batch count; exhaustion remains a failure, not permission to retry.
Model `gpt-6.1-sol` uses the existing metered egress: endpoint and key are
resolved only through `secret_ref` names `model-endpoint-9r` and
`model-key-9r-sol` in the owner's private secret store, never copied.

Raw redacted model responses, native CLI results, source gold, exact ordered
comparisons, usage and cleanup receipts remain under `evidence/<run-id>/`.
Numeric formatting is normalized losslessly; strings/NULL and row/column
order remain distinct. Truncated, refused, invalid, unverified, failed and
not-run answers cannot pass. A correct answer must follow a successful native
query; structured errors and nonzero exits remain visible. Setup/binding
failure records all 20 questions as not run, never as zero or passed.

Each run writes explicit snapshot verdicts through `verdicts.write_rows`.
T5 checks every table/column item in the retained Builder input against its
accepted draft and the reviewed output, including family inheritance; it is
scoped to the 12 gold tables, not full-fixture T5. Missing items or missing
review fail; the synthetic publisher skips T5. T6 passes only with all 20
distinct question IDs correct in that one run. Unmeasured source versions or
failed cleanup cannot pass. Both Builder egress and Agent egress use a
300-second timeout. No call or token limit is increased.

Merge runs without rerunning them:

```sh
tools/oo-test/oo-test matrix --merge-only --merge-evidence evidence/RUN
```

A timeout-only failed dialogue may be rerun once when authorized, with a new
run ID and its own Builder and all 20 answers. Never combine answers.
