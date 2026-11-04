# SQL Server product acceptance runner

```sh
tools/oo-test/oo-test matrix --engine sqlserver --versions 2019,2022,2025 \
  --suites T0,T1,T2,T7,T8
python3 -m unittest discover -s tools/oo-test -v
```

The default fixture has 2,000,000 events, 200 small tables and a 300-column
table. SQL Server uses the shared neutral generator's rows and shapes with
the dialect in `seed/preamble.sql`. Pins live beside the fixture in H's JSON
lock format; no coordinator-owned registry lock is changed. Runs are serial,
with one lane-E source and one lane-E PostgreSQL Agent DB at a time.

- T0 invokes the installed product `oo doctor`. Its source result must pass.
  The aggregate intentionally remains blocked by an absent model secret;
  no real model endpoint or credentials are used.
- T1 invokes `oo-load` while a separate admin connection repeatedly commits
  insert, update, delete and primary-key changes. Every table is compared
  with Agent DB using normalized row hashes. The large table is compared in
  10,000-key ranges. Duplicate rows and NULL versus empty remain distinct.
  Only the changing table may match either of two pre-recorded committed
  states. This does not claim Gateway publication.
- T2 starts separate `oo-load` processes for three C5, three CT and three CDC
  cycles. Each cycle compares source and target; idle cycles require zero
  logical target writes and zero new generations (also zero C5 payload).
  CT/CDC initial positions are measured before a quiescent aligned baseline.
  CDC alignment waits for a captured admin marker deletion, not a timer.
  Wrong-epoch attempts must fail without changing durable target state.
  Durable state binds database epoch, schema and mode. An 80-bit CDC LSN is
  stored losslessly as numeric file/offset; receipts retain the SQL Server
  opaque position. This encoding does not mean MySQL is involved.
- T7 is a separate quiescent CDC-read window after stopping the writer.
  Extended Events records the reader's top-level batches. Lost/truncated or
  empty audit, unrecognized statements, changed business rows or changed
  non-system object inventory fail. CDC-owned metadata/capture objects are
  individually inventoried and fingerprinted. The only permitted internal
  changes are append-only native `cdc.lsn_time_mapping` idle-scan rows:
  increasing 80-bit LSN, zero transaction ID and begin LSN, equal timestamps
  inside the measured window, unchanged existing rows/catalog identity, an
  enabled capture job, increasing empty-scan telemetry and zero scan errors.
  The test waits for a native append; it does not manufacture internal rows.
  Unknown CDC changes fail. This does not certify asynchronous capture-table
  writes under an active admin workload.
- T8 records sampled source connection count, connector read/pacing time,
  logical target bytes, and zero model calls. Connection observations are
  samples, not a continuous server-wide maximum. Reader pacing must supply
  at least three sleep nanoseconds per measured read nanosecond (25% duty).

Passwords and TLS private keys are generated into ignored private fixture
directories and removed on cleanup. Raw commands, failures, comparisons,
resource receipts and checksums remain under `evidence/<run-id>/`.
Reduced `--scale`/`--tables` runs are smoke tests, not full-scale acceptance.
Docker CLI retries are limited to three proven local pre-dispatch socket
refusals; every failed attempt remains evidence. SQL errors, product failures
and ambiguous command timeouts are never replayed automatically.
