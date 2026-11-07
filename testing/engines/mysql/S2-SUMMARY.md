# Lane C S2 partial evidence

## Digest primitives

`SourceEngine::digest_range` now sums both MD5 halves modulo 2^64 instead
of XOR. Equal duplicate pairs no longer cancel in no-key tables. The
`md5sum2:` algorithm tag deliberately differs from old receipts, so they
cannot authorize a no-change decision. Malformed aggregate fields fail
closed instead of becoming zero.

- Regression: `c-s2-digest-regression-20261104`, MySQL 8.4.7, original
  adapter fails the new duplicate-pair assertion with two identical all-zero
  XOR digests despite changed rows.
- Final: `python3 testing/engines/mysql/run.py --suite s2-digest --run-id
  c-s2-digest-20261104-live02` passes on 5.7.44 (amd64), 8.0.44, 8.4.7 and
  9.5.0. Lane H shared seed: 2,000 events, three small tables.
  `comparison-report.json` in each version reports equal source/adapter
  rows using lane H's comparator, with explicit quiescent consistency B.
- Earlier `live01` failures remain: the fixture database inherited latin1
  on 5.7, and the test capture helper queued split children behind later
  ranges. The fixture now declares utf8mb4; retries preserve source order.
  No gold values or equality assertions were relaxed.

This does not complete S2-C-02: the tests do not run a four-layer C5
scheduler, a target, generation publication, or a multi-source handoff.

## Watermark primitives

`python3 testing/engines/mysql/run.py --suite s2-watermark --run-id
c-s2-watermark-20261104-live01` passes on 5.7.44 (amd64), 8.0.44, 8.4.7
and 9.5.0.

- `earliest_binlog()` reads the actual first `SHOW BINARY LOGS` entry and
  returns file/position 4; rotation preserves it and fixture-admin purge
  changes it. Retention settings are not used to manufacture a cursor.
- GTID and GTID-off file/position watermarks pass. Without REPLICATION
  CLIENT the adapter returns no watermark and probe reports the grant.
- Each boundary rechecks replica channels and commit-order settings.
  OFF/missing settings refuse watermarks; probe returns a version-specific
  Requirement. ON permits the watermark. A previously observed primary
  becoming a configured replica is detected without reconstructing the engine.
- This uses a stopped, configured channel, not a running replica. It does
  not prove lag, log continuity, row correction or timestamp retention.

## Unfinished S2 and dependency boundary

S2-C-01 and S2-C-02 remain **not done**. No `ChangeSource` stub, fake stream,
or setting-derived earliest position was added. Continuous L0 staging,
chunk-window correction in product code, four-layer C5 scheduling, no-op
target/generation receipts, multi-source/binding handoff and new-generation
rebaseline tests remain required.

Evidence directory: `c-commit-s2-20261104`.

- `binlog-feature-probe.log`: compiling an import of
  `mysql_async::BinlogStream` in this connector fails E0432: the dependency's
  `binlog` feature is disabled. The temporary probe was removed.
- `binlog-lock-probe.log`: explicitly enabling `mysql_async/binlog` on the
  command line passes `cargo check -p oo-connectors --features
  mysql,mysql_async/binlog --locked --offline`. This feature alone needs no
  lock update, but normal builds need the coordinator-owned crate manifest
  changed before a decoder can be integrated.
- `preview-check.log`: `cargo check --manifest-path tools/mysql-preview/Cargo.toml
  --locked --offline` fails resolution: preview pins Tokio 1.53.1 while
  `oo-connectors` requires 1.53.3. Its `source-admission` dependency also
  pins 1.53.1 outside this card's write scope. The preview uses mysql_async
  0.36.1 while the connector uses 0.37.1. Reuse needs coordinated dependency
  alignment and standalone lock updates; no duplicate decoder was written.

`S2-prerequisites.patch` contains the exact requested manifest edits and
passes `git apply --check`. It is not applied and is only a prerequisite,
not an implementation of the remaining S2 work. The coordinator must update
the standalone locks and validate the preview/worker consumers after alignment.

No Cargo manifest/lock, pipeline, shared fixture, docs/specs/execution,
or engine-interface changes were made. All fixture resources were removed
after each run. Raw failed runs remain under ignored evidence.

## Local validation

`cargo check --workspace --locked` passes. MySQL-enabled connector unit tests
pass (179 passed, two explicit live ignores). Full
`cargo test -p oo-connectors --features mysql --locked --no-fail-fast` has one
failure: `parse_file_cli::parent_deadline_kills_the_worker_and_the_next_call_completes`.
It fails identically on integration base `ce4be6ad4`; see
`integration-base-parse.log` and `watermark-tests.log` in
`c-commit-s2-20261104`. The integration Darwin socket fix resolves the older
file-worker failures. Touched Rust files pass rustfmt checks; the unrelated
integration file's pre-existing formatting is left unchanged.

`python3 -m unittest discover -s tools/oo-test -q` passes.

## Final S0/S1 regression on the S2 code

`python3 testing/engines/mysql/run.py --run-id c-s2-regression-20261104-live01`
passes T0, adapter T1 and source-side T8 on all four versions:

| Version | Static rows | Canonical differences | Peak reader connections |
| --- | ---: | ---: | ---: |
| 5.7.44, amd64 | 2,000,000 | 0 | 1 |
| 8.0.44 | 2,000,000 | 0 | 1 |
| 8.4.7 | 2,000,000 | 0 | 1 |
| 9.5.0 | 2,000,000 | 0 | 1 |

The concurrent-writer/binlog-window fixture and duty-cycle assertions also
pass. This preserves the earlier harness scope: not production CDC or full
AT-ENG-01 acceptance.

Final artifact hashes for this regression and the two S2 primitive matrices:
`evidence/c-commit-s2-20261104/final-index.json`.
SHA-256: `318d388ffe5846c2d0f56c1fcf4d46d91871d6b730d0de41070b1e7621c2f41a`.
Final lane-C container and volume inventories are empty.
