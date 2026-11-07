# G-S5f: remaining P0 dialogue runs

STATUS: partial. S5-G-01 is not done. All nine requested versions were run;
six now have a new standalone 20/20 result. MySQL 8.0.44 and PostgreSQL
13.23/14.24 still lack a 20/20 run. No answers were combined across runs.

## Execution

Tested clean product revision: `50b625c71` (full SHA in every verdict row).
All fixtures ran sequentially, at most one source and one target database
created by this execution at once, with `oo.lane=g`. No unrelated resources
were stopped. All nine source versions ran natively, including pinned Oracle
Enterprise `19.19.0.0.0`; Agent DB remained PostgreSQL 17.

Before every run, including retries, the configured model-egress made calls
with `max_tokens=1` until three consecutive responses succeeded. Each gate
allowed at most 30 minutes, used a 60-second per-probe timeout, and persisted
only safe outcomes/usage. All 16 gates succeeded. A successful gate did not
prevent later upstream errors. The PostgreSQL 15.19 gate first saw
`UPSTREAM_ERROR`, then achieved the required consecutive successes.

Commands:

```sh
cargo check --workspace --locked
cargo build --locked -p oo-builder --bins -p oo-cli -p oo-gateway
CARGO_TARGET_DIR="$PWD/target" cargo build --locked --manifest-path tools/oo-load/Cargo.toml
CARGO_TARGET_DIR="$PWD/target" cargo build --locked --manifest-path tools/release-coordinator/Cargo.toml
python3 evidence/g-s5f-validation-20261105/runs.py
```

The last command invokes `probe.py` before each
`tools/oo-test/oo-test t6 --engine ENGINE --version VERSION --run-id ID --execute`.
Scripts, exact commands, probe logs, process results and diagnostics are retained
under validation run `g-s5f-validation-20261105`. No model, prompt, gold, timeout,
scoring, call budget or product implementation was changed.

Run IDs below use prefix `g-s5f-` and date suffix `-20261105`; a retry adds
`-retry` after the date. The session crossed local midnight into November 6;
verdicts retain actual UTC observation timestamps, not dates inferred from IDs.

| Engine/version ID stem | First run | One permitted retry |
|---|---|---|
| mysql-8-0-44 | 19/20; Q14 upstream timeout | 19/20; Q16 upstream timeout |
| mysql-8-4-7 | 19/20; Q06 upstream timeout | 20/20, exit 0 |
| mysql-9-5-0 | 20/20, exit 0 | not needed |
| postgres-13-23 | binding timeout; 20 not run | 19/20; Q19 upstream timeout |
| postgres-14-24 | 18/20; Q02/Q07 upstream timeout | Builder HTTP 502; 20 not run |
| postgres-15-19 | 19/20; Q03 upstream timeout | 20/20, exit 0 |
| postgres-16-15 | binding timeout; 20 not run | 20/20, exit 0 |
| postgres-18-6 | 17/20; Q07/Q09/Q18 upstream timeout | 20/20, exit 0 |
| oracle-19-19 | 20/20, exit 0 | not needed |

Every nonpassing command exits 1. Retries were admitted only after runs whose
failures were upstream timeouts; each retry started over with its own fixture,
Builder, review, publication, binding and all 20 questions.

The PostgreSQL 14.24 retry failed before dialogue:
`builder-baseline.stdout` ends with
`{"published":false,"reason":"s5_provider_http_502","stage":"failed"}`;
its Builder usage records `UPSTREAM_ERROR`. The harness reports an
`AssertionError` at the retained Builder subprocess boundary. This is not
20 incorrect answers, and the preceding expected closed-fence publication
refusal is not the cause. No third run was authorized or performed.

## MySQL 9.5 Q20 ownership

Historical run `g-s5e-mysql-9-5-0-20261105-retry/Q20.json` was verified against
its manifest (SHA-256
`13ec84e008981987d43995c51d55b2dd146c47ca2d29ed094ccc2446867251d5`).
The question asks for the count across January and February shards; gold is
`[["2"]]`. The Gateway rejected a derived-table query and UNION ALL with
`STATEMENT_NOT_ALLOWED`, accurately explaining unsupported subquery/set
operations. Two subsequent single-table queries succeeded, each returning
`[["1"]]`. The Agent returned `[["1"],["1"]]` instead of adding them.

Primary owner: the T6 model Agent's answer synthesis, driven by
`tools/oo-test/t6.py:dialogue`. Contributing capability boundary: Gateway SQL
validation. This evidence does not demonstrate a MySQL connector/data fault.
The prompt's instruction to preserve query row order may contribute, but that
causal explanation is not proven; no prompt patch or scoring relaxation was
applied.

Fresh run `g-s5f-mysql-9-5-0-20261105/Q20.json` encountered the same two SQL
refusals and the same two successful counts, then correctly returned `[["2"]]`.
Thus the unchanged rerun passes, but it does not prove deterministic recovery.
Both traces and the diagnosis are bound in `mysql95-q20-diagnosis.json`.

## Verdicts, verification and remaining limits

- All 10,098 files in the 16 fresh run manifests verified. `run-verification.json`
  records each result/manifest digest, probe gate and Builder outcome.
- All raw result files say `containers_left=0`; independent final lane-G
  container and volume scans are empty.
- `oo-test matrix --merge-only` validated and merged 32 fresh rows into
  `h-merged-bae6c465-86f3-4441-b962-b2184c06b8ad`. Exit 1 intentionally retains
  failures. Together with the separate historical backfill, 38 rows were added.
- `gen_support_matrix.py`, `--check`, `--self-test` pass. The generator retains
  failures and does not promote scoped gold-table T5 to full Self-build.
- `cargo check --workspace --locked` and all prerequisite locked builds pass.
- `python3 -m unittest discover -s tools/oo-test -v`: 131 tests pass.
- `check_spec.py`, `check_plan.py`, `check_v02_plan.py`: pass.

T5 completeness passes on 15 runs, but only for retained gold-question tables;
the PostgreSQL 14.24 retry has no completed Builder and a failing T5 row.
Full-fixture T5, T7, public/business acceptance and any new source-real-time
guarantee are not claimed. No Rust code or Cargo.lock changed, so Rust
formatting and crate test reruns are not applicable to these evidence-only
commits. No pre-existing test failure was waived.

Follow-up requires coordinator authorization for the exhausted versions and
the historical Oracle 23.5 cleanup failure. No further paid retries were
started. No additional shared-file patch is requested.
