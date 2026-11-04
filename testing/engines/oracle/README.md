# Oracle SourceEngine fixture (lane E)

Run from the worktree root, one version at a time:

```sh
python3 testing/engines/oracle/run.py --version 23 --run-id oracle-e-23
python3 testing/engines/oracle/run.py --version 21 --run-id oracle-e-21
python3 testing/engines/oracle/worker/build.py --run-id oracle-e-worker
python3 testing/engines/oracle/run.py --version 11 --run-id oracle-e-11 \
  --worker-image oo-e-oracle-worker:oracle-e-worker
cargo test -p oo-connectors --features oracle --lib --locked
```

The runner requires Docker and the locally available images
`gvenzl/oracle-free:23-slim`, `gvenzl/oracle-xe:21-slim`, and
`gvenzl/oracle-xe:11`. It resolves each local tag to its RepoDigest, records it,
and creates a new container from that digest. It does not pull, sign in,
connect to an existing instance, or reuse another lane's container.
23 runs as linux/arm64; 21 and 11 run as linux/amd64.

All created containers carry `oo.lane=e` and `oo.run_id`. At most two runner
invocations may be active at once. Ports bind only to loopback. Passwords are
generated into mode-0600, git-ignored files, never command arguments. Admin
SQL goes through stdin to the new container's `sqlplus -s / as sysdba`.
Only the fixture owner writes rows. The engine connects as a separate
SELECT/FLASHBACK reader. Cleanup addresses the created container by ID;
fixture bind directories and generated credential files are removed even
on test failure. A process SIGKILL still requires manual cleanup of that
run's labeled container and evidence-local data directories.

## Driver and client setup

23ai/21c use the existing pinned `oracledb = 26.0.0-beta.4` thin driver,
with default features disabled. **No host Instant Client, OCI library,
SQL*Plus installation, or library-path setting is required.** The fixture
uses the SQL*Plus binary already on PATH in each Oracle image, as the preview
tools use container-provided SQL*Plus. The old preview scripts contain
environment-specific paths and are not executed or reused as configuration.

The thin driver rejects 11g with `ServerVersionNotSupported`. Use
`OracleEngine::connect_worker` for the documented P2 path. The trusted caller
supplies a pinned worker command; the same engine SQL, Arrow canonicalization,
chunk receipts and budget controls apply to both transports. Versioned JSON
control travels over stdin, and stdout carries bounded length-prefixed JSON
status and Arrow IPC batches. Database errors preserve their numeric ORA code
without exposing vendor messages. Each Arrow batch carries its protocol
version, monotone sequence and validated schema SHA-256. Completion row counts
must match decoded rows. Missing completion is never success. Nested OCI
timeout codes remain budget failures even when wrapped by DPY-4011.

`worker/build.py` obtains Instant Client 19.30 only from Oracle's public
no-login download, validates its fixed SHA-256, records the supplied
`BASIC_LICENSE` (Oracle Free Use Terms and Conditions) in ignored evidence, and
builds a local-only amd64 Linux image with python-oracledb 3.4.2 in thick mode.
The Python base is digest-pinned. Do not publish this image or commit the
client archive; no redistribution authorization is implied. The runner resolves
the worker image to its content ID and records the runtime client version.
No Agent DB credentials enter the worker. `fixture_admin.py` is mounted
separately for fixture writes and is not included in the source worker image.
19c remains blocked by D2; this runner has no 19c database target.

## Coverage and boundaries

- T0 component: real version, settings, missing C1 prerequisites, DBA grants,
  online_catalog-only policy, key shapes, and plans. This is not `oo doctor`
  CLI integration. C1 reports the experimental reader/T7 acceptance gate.
- T1 component: two million NUMBER-key rows, VARCHAR2 case/accent/trailing
  spaces across chunks, composite and unique-only keys, extent-based heap
  ranges, growing ranges, and empty-string NULLs. A separate writer commits
  inserts, updates, deletes and composite-key moves during the read.
  The sink verifies canonical Arrow IPC byte count/SHA-256 and stages row
  hashes. Failed sinks are discarded before range splitting.
- SCN correction: for keyed ranges the harness obtains changed keys from
  Flashback Versions, replaces/removes their hashes using an independent
  full-table AS OF high-SCN reference, and compares the complete result.
  This exercises undo-bounded correction, **not LogMiner CDC or Agent DB
  loading**. Heap ranges are compared at their one plan SCN, never across
  snapshots.
- T8 component: the engine's one-session counter, measured read/sleep duty,
  an actual V$SESSION active-session pause, and a 1-ms timeout rejection.
  Agent DB write bytes, model tokens, and external connection-peak telemetry
  are outside this fixture.
- C5 regression: length-framed NULL-aware row hashes distinguish embedded
  delimiters; empty string and NULL retain Oracle semantics. 11g SQL uses
  seeded ORA_HASH, not unavailable STANDARD_HASH. Digest algorithm version
  is v2; old v1 digests require a fresh baseline.

`EXPLAIN PLAN` inserts into PLAN_TABLE and conflicts with source zero-write.
Instead, the adapter executes a bounded read and inspects that cursor's
V$SQL_PLAN through V$SESSION. Missing visibility is explicitly `Unavailable`,
not a verified index. DBA can grant SELECT on SYS.V_$SQL_PLAN and
SYS.V_$SESSION. Heap planning requires DBA_EXTENTS; without visibility the
plan explicitly falls back to a bounded single read. No parallel-execute
tasks or other source objects are created.

Current limitations (do not upgrade the support matrix from these tests):

- Non-binary session-bound key ordering is rejected rather than forced with
  NLSSORT. Data-bound collations use source comparisons but need broader
  live NLS coverage.
- `SourceEngine::read_chunk` retains the same SCN on bounded ORA-01555
  page retries and fails closed when undo is lost. Callers with chunk-local,
  unpublished staging can use `OracleEngine::read_chunk_recovering` and
  `RetryableChunkSink`: after exhaustion it discards the entire failed chunk
  and restarts from its lower bound at a fresh SCN, never from the old cursor.
  The successful receipt brackets the new snapshot. Correct its log window
  or verify its range with C5 before publishing. The generic pipeline does
  not yet opt into this Oracle-specific recovery API.
  Physical ORA-01555 still fails with
  `ora_01555_physical_replan`. The separate `--undo-stress` fixture installs
  an 8 MiB, non-autoextending undo tablespace with retention 1 second. It
  pauses after a flushed batch while a separate writer churns future rows.
  The test requires a real ORA-01555, successful fresh-SCN recovery of all
  12,000 rows, and zero diff against an independent high-SCN read. A key
  moves behind the old cursor to catch partial-resume bugs; a stopped-writer
  C5 digest recheck also runs. Failure exhaustion is no longer a pass.
- Heap chunks can still exceed a row budget on unusually dense blocks and
  fail closed. They cannot use the keyed split API.
- Unsupported types and virtual columns fail admission; no silent column
  omission. Arrow columns contain lossless canonical text and Oracle-type
  metadata, not the full native Arrow type mapping.
- Calls are synchronous inside async methods. Use a blocking-capable worker;
  cancellation cannot preempt a driver call before its client timeout.
- Planning/digest/read calls are paced; the synchronous `split_chunk` and
  `changed_keys` helpers have call timeouts but no independent duty-cycle
  wrapper. Retention-based admission, sampling above 1000 chunks, production CDC correction remain follow-up work.

Raw logs, image identity, plans, probe, test exit status, and SHA256SUMS are
under `evidence/<run-id>/<version>/`. Failed attempts remain evidence.
Only this documentation and the separate evidence summary are committed.

For the destructive undo stress test, **only** against a newly created fixture:

```sh
python3 testing/engines/oracle/run.py --version 23 --run-id oracle-e-undo --undo-stress
python3 testing/engines/oracle/worker/test_worker.py
```

## S2 incremental component tests

```sh
python3 testing/engines/oracle/run.py --version 23 --run-id oracle-e-s2-23 --incremental
python3 testing/engines/oracle/run.py --version 21 --run-id oracle-e-s2-21 --incremental
python3 testing/engines/oracle/run.py --version 11 --run-id oracle-e-s2-11 \
  --incremental --worker-image oo-e-oracle-worker:oracle-e-worker
```

`OracleLogMiner` implements `ChangeSource` over the existing engine transport,
column metadata, canonicalization, Arrow schema and key conversion. The old
preview C1 producer consumes Debezium envelopes; it contains no platform-owned
LogMiner reader to call. Its sealing/native-loader contracts are not duplicated
or replaced by this component. The existing native Oracle loader is unchanged.

The adapter admits keyed NUMBER/CHAR/VARCHAR2 tables with conventional uppercase
identifiers. Other types fail admission. CDB mining uses a common user in
CDB$ROOT, a matching PDB engine for metadata admission, and SRC_CON_NAME filtering.
It adds retained files itself and uses only DICT_FROM_ONLINE_CATALOG plus
COMMITTED_DATA_ONLY; it never executes SQL_REDO or builds a dictionary.
The oldest active transaction at admission pins the mining start. Each poll
replays from that start and filters by commit SCN, preserving transactions that
straddle a commit watermark. Missing logs, changed incarnation, and a new
process attempting an old checkpoint require rebaseline. There is no durable
in-flight transaction store or automatic retirement of that pinned start.
Multiple redo threads/RAC are refused: admission currently observes this
instance's V$TRANSACTION, not a cluster-wide active-transaction inventory.
Dictionary failures identify the affected tables and stop watermark advancement.

The scoped S2 harness is `s2.rs`, compiled under the Oracle engine library's
ignored tests. The runner selects `s2::real_incremental_cycles`; no shared
integration-test file or workspace manifest is changed. It compares a real
snapshot corrected with LogMiner in-window insert/update/delete/key-move events,
a transaction spanning polls, and an empty cycle. DDL must request rebaseline.

`OracleC5` provides an Oracle-local, in-memory four-layer scheduler for keyed
tables: optional per-range time hints reorder work; recent Flashback Versions
windows prioritize changed ranges; every range is digested regardless of hints;
a configurable periodic full read checks canonical content independently of the
digest. NULL/stale time hints never exclude rows or deletes. Physical plans are
rejected because their pinned snapshot cannot track new changes. Ordinary
unchanged cycles transfer no row payload, rewrite nothing, and keep generation.
Periodic verification full reads incur source read cost even when unchanged;
they do not rewrite or create a generation. This is component evidence, not
native Agent DB loading, durable recovery, or production publication.

The thick worker supports a separate `logminer` operation with exactly four
bound package statements (NEW/ADDFILE, START with online catalog + committed
only, END). It does not accept arbitrary PL/SQL, DML, DDL, dictionary builds or
redo execution. The fixture mounts the current `worker.py` read-only into the
existing pinned local worker image; the image's OCI/Python dependencies remain
unchanged and recorded. Fixture writes still use the separate admin script.

D11 is disclosed as an unconditional Feature requirement on LogCdc: explicit
opt-in and version-specific T7 acceptance remain necessary; auto must remain C5.
The fixture collects source audits plus SQL trace recursion and retains every
classification. Missing depth is not inferred from an object name. The original
41 exceptional 23-family audit rows are retained in `fixtures/d11-original-41.json`;
they lack depth/owner, so only the driver setting can be admitted without new
trace evidence. Tests separately exercise explicit recursive-depth cases and
reject top-level or foreign-object writes. Historical failures are not replaced.

21c XE starts with mixed auditing (`Unified Auditing=FALSE`, `audit_trail=NONE`).
The owned fixture sets `AUDIT_TRAIL='DB','EXTENDED'` in its SPFILE and restarts it,
then enables a unified policy for the miner. Both audit views are collected;
an empty audit never passes. 11g uses conventional extended auditing. The
23-family pure-unified image rejects new conventional AUDIT configuration, so
only a unified policy is created there. SQL trace is enabled only inside the
new disposable database, after all credential-bearing fixture DDL.

11.2 C1 now passes functional cycles, but its recursive deletes target
`SYSTEM.LOGMNR_GT_*`, outside D11's explicit `SYS.LOGMNR_GT_*` category.
21c also uses the SYSTEM-owned temporary objects and executes an update of
`SYSTEM.LOGMNR_SESSION$`. These are real T7 failures under the supplied D11
allowlist, not permission to widen it. Conventional
auditing also produces recursive `SYS.AUD$` writes in the session trace; these
are retained and not granted an undocumented exemption. The version evidence
summary separates function success from T7 and remaining unsupported cases.
19c remains blocked by D2. No support-matrix promotion is justified.

An extra probe-first 23-family attempt fails planning with
`ora_snapshot_unavailable` (both `e-s2b-final23` and its retry). The component
harness records `probe()` after the functional cycles; it does not establish
probe-then-snapshot/onboarding acceptance. This remains a separate follow-up,
not a passing rerun of that order. Full trace additionally reveals database
maintenance/auditing writes; these are not reader-issued application DML, but
are retained as failures under the card's no-other-object rule. Objectless
COMMIT records are not assigned a fictitious temporary-table scope.

Version qualification: the local `gvenzl/oracle-free:23-slim` digest used by
these S2 runs reports `23.26.3.0.0` and an Oracle AI Database **26ai** banner.
The run name `23` denotes the version-family/image target, not proof against an
older 23ai-branded build. Consult the pinned image and raw startup/probe output.

## S2c: D11a and strict C5 (2026-11-04)

`--incremental --c5-only` now configures a separate OO_READER audit without
creating a miner or enabling LogMiner. It records both audit modes and traces
inside the reader's PDB (root-only tracing misses PDB sessions). Read-only
capability checks against inaccessible LogMiner views are still SELECTs, not
LogMiner invocations. Audit collection is mandatory and empty evidence fails.

D11a admits DML only on SYS/SYSTEM LOGMNR-prefixed objects, and COMMIT only
when every observed execution is enclosed by a LogMiner call/query in the
same traced session. The fixture records conventional and unified audit
session IDs alongside SID/serial for correlation; SQL equality alone cannot
establish session identity. Known top-level writes, driver ALTER SESSION,
foreign-object writes and unproven recursive blocks remain failures. The
original 41-row fixture and prior failed reports are preserved.

The component harness now calls probe before planning and reading. On the
pinned 23.26.3 image, immediate index-plan flashback can fail with ORA-01466.
The index-only sample retries at a fresh SCN, at most `snapshot_retries` times,
with a one-second delay. No sample rows reach a sink; actual chunk snapshot
semantics are unchanged. The pinned thin driver's statement cache is disabled:
the retry-only experiment produced ORA-01003, whereas the uncached bounded
retry completed with a verified index and zero-diff cycles. The final 23 C1
run exercised one retry. Earlier unsuccessful experiments remain evidence.

This is not T7 acceptance. The thin driver emits ALTER SESSION during login;
the thick 11g path also shows session settings in trace. Strict C5 additionally
observes recursive audit/maintenance writes. These are not silently exempted
or attributed to the audit facility merely from their object names. The
S2c summary records every rejected SQL text and its audit/trace attribution.
Neither C1 nor C5 is promoted to verified zero-write status.

Final live run IDs are `e-s2c-{c1,c5}-{23,21,11}-final`. All six component
runs pass; their T7 verdicts fail. A separate `e-s2c-adjudication` report
reclassifies the preserved records with two evidence-backed distinctions:
unified SELECT labels on trace-correlated recursive DELETE, and 11g LOGOFF
BY CLEANUP lifecycle records. It never overwrites a live report. Final
rejected record counts (audit and trace records, not unique operations) are:

| Version | C5 strict T7 | C1 D11a T7 |
| --- | --- | --- |
| 23.26.3 | FAIL: 14 records | FAIL: 51 records |
| 21.3 | FAIL: 15 records | FAIL: 36 records |
| 11.2 | FAIL: 18 records | FAIL: 23 records |

`s2c-evidence-summary.json` contains the exact
remaining SQL, session/user/owner/depth fields where available, raw record
indices, source hashes and artifact hashes. For reproduction, the ignored
`evidence/e-s2c-adjudication/reclassify.py` records the read-only adjudication.
The full connector suite has one unrelated macOS failure:
`parse_file_cli::parent_deadline_kills_the_worker_and_the_next_call_completes`
reports `group_signal=rejected` rather than `sent`. The same test fails on
archived base `f27d1a2c0`; with that one test filtered, 253 pass and 22 are
ignored. Ignored database tests are not counted as real engine passes.

## S2d: D12 attribution (2026-11-04)

`s2d-d12-evidence-summary.json` records new live C5 and C1 runs on 23.26.3,
21.3 and 11.2: all six component tests and T7 classifications pass. This
supersedes the strict D11/D11a verdict for these new runs only; historical
failed evidence above is unchanged.

The classifier lists session settings, recursive system login/dictionary
bookkeeping, system audit trails, and D11a LogMiner objects/internal blocks
separately. It rejects top-level writes, unknown calls, foreign objects and
missing attribution. Comment-prefixed blank-object INSERTs are resolved from
the trace SQL target, parser UID, recursion depth and SQL ID, never ignored.
Internal LogMiner blocks use exact reviewed SQL plus an enclosing same-session
LogMiner execution, not a package-name prefix allowance. C5 has its own
non-LogMiner reader and audit.

These are source component/T7 results, not durable Agent DB publication.
19c remains untested; C1 still requires explicit opt-in.

## Bounded LOB snapshots

CLOB/NCLOB locators are read incrementally into canonical UTF-8; BLOB locators
become lowercase hex, following the adapter's existing canonical Arrow
`Utf8` contract. Fields retain their Oracle type and canonical-format metadata.
Empty locators remain empty values, distinct from NULL. LOBs cannot be keys.

Both thin and 11g thick transports use the remaining `ReadBudget` chunk bytes
as their limit (including hex expansion), with bounded locator reads and a
deadline. Over-limit values fail with `lob_value_over_budget`; no truncated
value or successful receipt is returned. A caller must discard unpublished
chunk staging on any error, as with other chunk failures. Source SQL digest
of LOBs remains explicitly unsupported (`lob_source_digest_requires_streaming`);
this does not disable snapshot Arrow IPC SHA-256 verification.

Run the focused live test with `run.py --version <23|21|11> --run-id <id>
--lob-only`, adding the pinned thick worker image for 11g. It checks multichunk
CLOB/NCLOB/BLOB content, Unicode, empty/NULL values, receipt SHA-256 and a
4096-byte overflow refusal. Fixtures use disposable tmpfs rather than host
database bind directories.
