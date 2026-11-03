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
  CLI integration. C1 always reports the S2 reader/T7 acceptance requirement.
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
- ORA-01555 retries are bounded, shrink keyed pages, retain the same SCN,
  and resume only after a successfully flushed key. Lost undo cannot be
  repaired by shrinking; exhaustion fails. Physical ORA-01555 fails with
  `ora_01555_physical_replan`. The separate `--undo-stress` fixture installs
  an 8 MiB, non-autoextending undo tablespace with retention 1 second. It
  pauses after a flushed batch while a separate writer churns future rows.
  Its result distinguishes full same-SCN recovery from safe retry exhaustion;
  a passing fail-closed assertion is **not** recovery acceptance.
- Heap chunks can still exceed a row budget on unusually dense blocks and
  fail closed. They cannot use the keyed split API.
- Unsupported types and virtual columns fail admission; no silent column
  omission. Arrow columns contain lossless canonical text and Oracle-type
  metadata, not the full native Arrow type mapping.
- Calls are synchronous inside async methods. Use a blocking-capable worker;
  cancellation cannot preempt a driver call before its client timeout.
- Planning/digest/read calls are paced; the synchronous `split_chunk` and
  `changed_keys` helpers have call timeouts but no independent duty-cycle
  wrapper. Retention-based admission, sampling above 1000 chunks, T7 audit,
  and production CDC correction remain follow-up work.

Raw logs, image identity, plans, probe, test exit status, and SHA256SUMS are
under `evidence/<run-id>/<version>/`. Failed attempts remain evidence.
Only this documentation and the separate evidence summary are committed.

For the destructive undo stress test, **only** against a newly created fixture:

```sh
python3 testing/engines/oracle/run.py --version 23 --run-id oracle-e-undo --undo-stress
python3 testing/engines/oracle/worker/test_worker.py
```
