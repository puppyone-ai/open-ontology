# T10 harness handoff (S6-H-02 / D24)

## Scope and result

Offline harness implementation only. No Docker commands were executed against
this host, no containers were created, and no live engine result is claimed.
The complete default phase selection is **blocked** until the coordinator applies
`tools/oo-test/T10-duty-cycle.patch` to `tools/oo-load/src/main.rs`, adds CLI
boundary tests (1/100 accepted; 0/101/non-number rejected; default unchanged),
and rebuilds. This is an exact proposed diff, not an applied product change.
The loader currently exposes no duty override. T10 refuses before resource
creation rather than mislabeling a default-duty measurement. Other phases can
be selected with `--phases baseline,initial,c1,c5` without that patch.

## CLI and phases

```
python3 tools/oo-test/t10.py --engine postgres|mysql|oracle --version V \
  --phases baseline,initial,initial_duty100,c1,c5 --minutes 10 \
  --rate auto --cpus 2 --memory 4g --evidence evidence/NEW-RUN \
  --binary tools/oo-load/target/release/oo-load
```

- Source startup reuses T3/T4's T1-derived pinned images, admin provisioning,
  TLS, read-only product roles, and ownership-checked cleanup. Both database
  containers/volumes carry `oo.lane=t10`. Only source CPU/memory is constrained;
  actual Docker limits and version/image/platform are recorded. At most one
  source and one target exist concurrently. No other running container is
  allowed at preflight, and none is stopped by T10.
- The narrow T3/T4 `bulk` fixture is enlarged to 2,000,000 rows (full T1 row
  scale, not T1's type-coverage dataset). Foreground uses separate writable
  connections and a deterministic 60% point SELECT / 30% hot UPDATE / 10%
  INSERT mix on `items` / `other` throughout. Product uses only reader roles.
- Auto calibration ramps closed-loop concurrency 1..128 in 10-second steps,
  requires a measured throughput plateau, and selects 55% of the best rate.
  Calibration errors or an unproven capacity at the cap refuse the run. Explicit
  `--rate R` skips calibration and is disclosed. Each ramp closes its connections.
- `baseline`: foreground only for `--minutes` (default 10).
- `initial`: fresh Agent DB, default oo-load settings, measured to completion.
- `initial_duty100`: another fresh Agent DB, only duty changed to 100; every
  load receipt must confirm 100. Requires the coordinator patch.
- `c1`: an unmeasured quiescent baseline and explicit native C1 binding, followed
  by continuous bounded oo-load sync calls during the foreground interval.
  MySQL requires the `mysql-binlog` build feature; Oracle uses explicit LogMiner
  opt-in. PostgreSQL slots are retired after this phase.
- `c5`: two independently measured cycles using the checked-out lite scheduler's
  `c5_full_read_every({})` default (currently 24), no chunk/duty override.
  The idle binding scans unchanged `bulk`; foreground still writes `items` and
  `other`, outside that binding. The changed binding includes all three tables,
  is explicitly dirtied, and receives foreground writes during reconciliation.
  This scope distinction is disclosed, not presented as an all-tables idle run.

## Measurements and verdicts

The open-loop scheduler records intended start, actual start, completion,
end-to-end latency, service time and sanitized error class per request. A bounded
queue reports every rejected arrival as an error. Queued work is drained, but
late completions do not inflate achieved throughput inside the arrival interval.
`observation_seconds` includes drain/product shutdown and the metric-sampling
tail; `duration_seconds` is the foreground arrival interval.
Ten-second windows use arrival cohorts for latency and completion time for rate;
the final partial window uses its actual duration. Percentiles interpolate exact
sorted samples, including queueing delay (no coordinated-omission correction is
needed because arrivals are not gated on completions).

Every ten seconds a separate admin sampler collects Docker CPU/block IO and
engine metrics. PostgreSQL: reader xmin age, hot-table dead tuples/autovacuum,
retained slot WAL. MySQL: history-list length via `INNODB_METRICS`, running threads,
and history-list length explicitly labeled as purge backlog, **not time lag**.
Oracle: undo blocks, tuned undo retention, snapshot-too-old count and sampled
live LogMiner-session CPU. Undo counters cover the retained V$UNDOSTAT horizon;
CPU from already-exited miner sessions can be missed. Peaks and first-to-last
changes are reported; these are not automatically causal attribution.

Each phase JSON contains p50/p95/p99, rate, errors, windows, metric samples/peaks,
product errors, and its D24 verdict. Missing metrics, invalid baseline, product
failure, missing requested phases or cleanup failure cannot produce an overall
pass. D24 is p95 <= +20%, p99 <= +50%, throughput >= -5%, zero errors. Raw requests,
commands and outputs stay under ignored `evidence/`. `summary.json` and
`t10-index-rows.json` bind explicit phase verdicts to the summary SHA-256.
The existing T0..T8 index reader is unchanged; coordinator ingestion must handle
these separate T10 rows, not pass them to its closed T0..T8 validator.

## Offline evidence

Run ID: `t10-harness-20261108`.

- `python3 -m unittest discover -s tools/oo-test -p test_t10.py -v`: 33/33 pass
  (`unit-verified-r2.*`). Math, scheduling, saturation/error handling, calibration,
  stdout-format samples for three engines, CLI, limits/labels, product failure,
  default flags and duty receipt refusal are covered. Samples are static format
  fixtures, not new live measurements. Initial test-mock failure is preserved in
  `unit.*`; fixed without changing the implementation's budget or gold values.
- `python3 -m py_compile tools/oo-test/t10.py tools/oo-test/test_t10.py`: pass
  (`compile-verified-r2.*`). CLI `--help`: pass (`cli-help.*`).
- `cargo check --workspace --locked`: pass (`cargo-check-verified.*`). No Rust crate or
  lockfile changed; cargo fmt and touched-crate tests are not applicable.
- `PYTHONPATH=tools/oo-test python3 -m unittest test_t1 test_t2 test_t3t4 test_oracle_t1 -v`:
  38 pass, one pre-existing error:
  `test_t3t4.T3T4.test_oracle_ddl_uses_portable_dialect_and_both_widenings`,
  `ValueError: widen_string`. Reproduced by running that exact test from files
  extracted with `git archive 44c498144fcf2cceda5d5d759169472e3d4f32f8`
  under this worktree's ignored evidence directory (`base-test-r2.*`). The first
  incomplete extraction/import failure is preserved in `base-test.*`.
- `git apply --check tools/oo-test/T10-duty-cycle.patch`: pass (not applied).

## Exact future live commands (not executed)

Only after the other lane has finished, the host is quiet, and the coordinator
has integrated/tested the duty patch. Run sequentially in the priority order
below. Each evidence directory must be new; do not overwrite failed runs.

```sh
mkdir -p evidence/t10-runtime
python3 -m venv evidence/t10-runtime/venv
evidence/t10-runtime/venv/bin/pip install 'psycopg[binary]' 'pymysql[rsa]' oracledb
cargo build --manifest-path tools/oo-load/Cargo.toml --release --locked --features mysql-binlog

evidence/t10-runtime/venv/bin/python tools/oo-test/t10.py --engine oracle --version 23 --phases baseline,initial,initial_duty100,c1,c5 --minutes 10 --rate auto --cpus 2 --memory 4g --evidence evidence/t10-live-oracle23 --binary tools/oo-load/target/release/oo-load
evidence/t10-runtime/venv/bin/python tools/oo-test/t10.py --engine mysql --version 8.4 --phases baseline,initial,initial_duty100,c1,c5 --minutes 10 --rate auto --cpus 2 --memory 4g --evidence evidence/t10-live-mysql84 --binary tools/oo-load/target/release/oo-load
evidence/t10-runtime/venv/bin/python tools/oo-test/t10.py --engine postgres --version 17 --phases baseline,initial,initial_duty100,c1,c5 --minutes 10 --rate auto --cpus 2 --memory 4g --evidence evidence/t10-live-postgres17 --binary tools/oo-load/target/release/oo-load
```

Live driver/SQL compatibility, actual capacity, D24 outcomes, and mitigations
remain for the later live card. No mitigation is justified by these unit tests.
