# TA-01 initial timing slice

Status: partial. S6-H-01 is not complete. No accelerated layer is enabled.

## Evidence

Run ID: `ta-01-timing`. Base: `eb301aa86`.
Raw outputs remain under the ignored `evidence/ta-01-timing/` directory.

| Check | Result |
| --- | --- |
| `CARGO_BUILD_JOBS=2 cargo check --workspace --locked` | pass, initial and final runs |
| `PYTHONDONTWRITEBYTECODE=1 python3 -m unittest discover -s tools/oo-test -v` | pass, 215 tests, 5.015 seconds |
| `tools/oo-test/oo-test timing --help` | pass |
| `python3 specs/scripts/check_spec.py` | pass, document integrity only |
| `python3 specs/scripts/check_plan.py` | pass, document integrity only |
| `python3 specs/scripts/check_v02_plan.py` | pass, document integrity only |

Eleven new timing tests cover nested exclusivity, missing stages, exceptions
and interruption, receipt preservation, subclass overrides, inherited nesting,
harness decorators, native product command selection, missing/empty receipts,
summary deduplication, path rejection and cleanup failure evidence.
No Rust source or manifest changed; crate formatting is not applicable.

## Deliverable exits

1. Timing: partial. Common source lifecycle, T1 target/comparison, T2 bulk seed,
   T3/T4 target/comparison, inherited T7 boundaries and synchronous native
   commands are instrumented. The README enumerates remaining boundaries.
   Missing stages remain null; timer exceptions do not replace suite verdicts.
2. Snapshots: not implemented or validated on any engine/version.
3. Shared sources and destructive clones: not implemented.
4. Small fixtures and branch-coverage differential: not implemented or run.
5. Defect replay: not implemented or run.
6. L0/L1/L2, rotation, probes, impact selection and scheduler: not implemented.
7. Failpoints/toxiproxy and release-build exclusion: not implemented or run.
8. Host acceptance and CI wiring: not implemented or run.

The spec's before figure is approximately 5-6 hours, not a measurement made
by this executor. After wall time and all live stage baselines are unmeasured.
The 5.015-second Python run is harness unit testing, not L0 or fast acceptance.
Full and fast product line/branch coverage numbers are unmeasured.
There is no same-SHA fast/full verdict comparison.

## Defect gate inventory

| Required mutation | Caught / missed |
| --- | --- |
| Oracle worker `map_driver`, 5c0fadda4 | not run |
| C1 run-ID replay | not run |
| MySQL 5.7 latin1 / RENAME | not run |
| Decimal256 carry, 127479529 | not run |
| PG parallel worker audit, 5d44ce476 | not run |
| T8 short-window duty, 4d23e6c51 | not run |
| Publication identity 0135/0136 | not run |
| Isolated stale grammar | not run |
| Over-broad audit grants | not run |
| Adjudication summary hash | not run |

Zero mutations were executed; this is neither zero misses nor a passing gate.

## Boundaries and continuation

The preflight records no `s6h01c` receipt tree or `.profdata` coverage baseline
in this worktree's evidence directory. The referenced sibling worktree was
not accessed under card rule 1. A coordinator-provided, SHA-bound baseline
inside this worktree is needed for comparison with that historical run.
The local absence of a baseline does not establish that none exists elsewhere.

No database/version acceptance ran. In particular, no PG 13-18, MySQL
5.7-9.5 or Oracle 11g/19c/21c/23ai acceleration claim is made.
No containers, volumes or images were created, changed or removed.
The observed lane-T fixture and unrelated services were left alone.
No paid model calls, secret-store access, service changes or external
worktree writes occurred. The pre-existing untracked dispatch script is
unchanged and excluded from the commit.

Next: finish all runner stage boundaries before establishing live baselines;
then implement snapshots and the subsequent steps in card order. Retain the
spec's replay/coverage/differential gates rather than enabling fast CI
on the strength of the harness tests here.

Requested shared-file patches: none. Cargo.lock changes: none.
