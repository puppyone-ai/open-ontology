# M-S2 author validation (partial)

This delivery does not close M-S2, AT-ENG-01 across all engines, or the S2
sprint. It supplies a typed durable target boundary and PostgreSQL C1/C5 CLI
and real-cycle runner. No Gateway publication or business acceptance is claimed.

## PostgreSQL matrix

Command: `tools/oo-test/oo-test t2 --versions 13,14,15,16,17,18 --run-id m-s2-pg-acceptance-20261104`.
Final binary includes pre-commit and feedback lineage checks. Each fixture uses
one pinned, lane-M-labelled database container with separate source/Agent DBs.

| Image version | C1 cycles | C5 cycles | Maximum sampled duty | T2 subset |
|---|---|---|---|---|
| 13.23-bookworm | 3/3 | 3/3 | 7.67% | passed |
| 14.23-bookworm | 3/3 | 3/3 | 14.62% | passed |
| 15.18-bookworm | 3/3 | 3/3 | 2.09% | passed |
| 16.14-bookworm | 3/3 | 3/3 | 5.64% | passed |
| 17.10-bookworm | 3/3 | 3/3 | 9.38% | passed |
| 18.4-bookworm | 3/3 | 3/3 | 12.55% | passed |

All cycles compare both fixture tables exactly against the source, including
insert/update/delete/key changes and multi-table source transactions. C1
positions are monotonic. No-change cycles have zero logical target tuple bytes,
zero new generations and unchanged total target relation bytes; C5 additionally
has zero delivered replacement payload. T8 samples pass the unchanged 25% duty
and two-reader-connection budgets. Every version also rejects an expired slot
without modifying target state and rejects a recreated source relation.

`handoff: not_implemented` remains explicit. The fixture is compact (bigint/text),
not full type/scale coverage or T7. Source C5 hints and full-read cadence are
caller-supplied; no autonomous scheduler or persistent cycle-number policy was
added. A changed range plan fails with explicit rebaseline rather than silently
refreshing the table. C1 requires a caller-asserted already aligned baseline;
production bootstrap/window repair is not implemented.

## Target and regression checks

- `cargo check --workspace --locked`: passed.
- `cargo test -p oo-pipeline --locked`: 349 passed, 53 ignored, zero failed.
- `cargo test --manifest-path tools/oo-load/Cargo.toml --locked`: 6 passed,
  6 ignored, zero failed. Ignored tests are not counted as passes.
- `python3 -m unittest discover -s tools/oo-test -p 'test_*.py'`: 34 passed.
- `python3 tools/oo-load/test-sync-target.py --run-id m-s2-target-20261104-final`:
  real PostgreSQL target passed atomic rollback, stale-position rejection,
  tombstones/key changes, mismatched-image-key rejection, lineage rejection,
  immutable parent reuse and no-change metering. Actual SIGKILL after target
  commit resumes without duplicate apply or changed position/generation/bytes.
- `check_spec.py`, `check_plan.py`, `check_v02_plan.py`: passed.
- `python3 tools/oo-load/test-target.py --run-id m-s2-snapshot-regression-20261104`:
  existing snapshot/empty/unsigned/replay regression passed, including both
  actual SIGKILL points (stage commit and publication commit).
- `git apply --check tools/oo-load/ENGINE-API-REQUESTS.patch`: passed; not applied.

Raw logs: `evidence/m-s2-20261104/`. Raw target evidence:
`evidence/m-s2-target-20261104-final/`. Per-version raw commands, image digests,
comparisons, T8 samples, negative checks and cleanup: the matrix run above.
Every successful fixture cleanup reports an empty failure list.

## Preserved failures

- `m-s2-pg17-20261104-a`: fixture lacked measurable WAL retention headroom;
  corrected admin fixture configuration, not source preflight rules.
- `m-s2-pg17-20261104-b`: C1 row comparisons passed but a T8 cycle measured
  31.45% duty. Coordinator metadata pacing was added; budget stayed 25%.
- `m-s2-pg17-20261104-c`: all C1 cycles passed; C5 fixture source ID contained a
  hyphen rejected by the existing SourceId contract. Changed fixture ID only.
- `m-s2-pg17-20261104-d`: Docker socket refusal interrupted the admin writer.
  Replaced repeated Docker exec writes with one persistent fixture-admin script;
  no uncertain write was retried.
- Concurrent C5 `c5_range_changed_retry` failures remain in final raw logs.
  They do not count as passing cycles; subsequent complete cycles must pass
  exact comparison before the runner marks a cycle passed.

## Outstanding card exits and owner requests

- S2-C-01: not done. No MySQL C1 Agent DB integration or version matrix run.
- S2-C-02: not done. No MySQL C5 adapter or C5-to-C1 handoff/barrier.
- S2-D-01: PostgreSQL T2 subset done; coordinator owns broader engine acceptance.
- S2-E-01: not done. No Oracle C5/opt-in C1 Agent DB integration or matrix run.
- S2 sprint exit / full AT-ENG-01: not done.

Exact, unapplied engine-owner accessor requests are in
`ENGINE-API-REQUESTS.patch`: expose the already checked MySQL server UUID and
Oracle DBID/resetlogs epoch for persistence and restart comparison. These small
patches alone do not complete the missing adapters. MySQL C5 baseline/proposal
state is private and process-local; Oracle C5 advances its in-memory baseline
before target confirmation. Durable prepare/confirm/checkpoint integration still
needs engine-owner work or a separately reviewed local adapter. ChangeBatch has
no authenticated epoch/feedback contract; the PostgreSQL implementation uses
explicit control queries and rejects detected lineage changes, not a claim of
transparent failover safety.

No root/shared Cargo files or existing migrations changed. New migrations are
0120 and 0121. Both Cargo.lock files remain unchanged. No customer systems,
other worktrees, tokens, unrelated services, pushes or PRs were touched.

## Evidence hashes

- `evidence/m-s2-pg-acceptance-20261104/matrix.json`: `7a4af777ea0186aed15c2b296933402a336130c5f649fe3353c2d0a6e2420d43`
- `evidence/m-s2-target-20261104-final/result.json`: `7f07379cef9807a2c955595c52d952324a8cabd496c75f94890f8837d9001dad`
