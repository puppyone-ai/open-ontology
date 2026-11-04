# G-S5d: single MySQL rerun blocked before dialogue

STATUS: blocked. S5-G-01 remains **not done**. The requested unchanged
MySQL command ran exactly once, exiting 1 during fixture setup. No earlier
run's answers are carried forward.

Tested revision: `d02b9eb22` (full revision in validation evidence).
Harness, gold, model configuration, budgets and timeouts are unchanged.

```sh
tools/oo-test/oo-test t6 --engine mysql --run-id g-s5d-mysql-20261105 --execute
```

## Result and blocker

- Source measured MySQL **8.4.7**; default scale 2,000,000 events and 200
  small tables; reviewed publisher selected.
- Setup reached the bootstrap role-pack command, then raised
  `FileNotFoundError`: `target/debug/oo-builder` does not exist.
  `test-gateway-cycles.py` selects that executable; the traceback reaches
  `Fixture.pack` and `GoldFixture.command` at `subprocess.run`.
- `oo`, `oo-load`, `oo-gateway` and `release-coordinator` executables are
  also absent under `target/debug`. The executor failed to verify these
  prerequisites before using the one permitted invocation.
- Agent calls: 0. Retained model Builder was not reached; the harness
  records Builder/total calls as null, not a measured count.
- No owner review, publication, Agent binding or dialogue was reached.
  This is a local test prerequisite failure, not an upstream timeout or
  a wrong model answer.
- No retry was attempted, respecting the once-only instruction.

## All 20 question results

| Question | This run |
|---|---|
| Q01 | not_run |
| Q02 | not_run |
| Q03 | not_run |
| Q04 | not_run |
| Q05 | not_run |
| Q06 | not_run |
| Q07 | not_run |
| Q08 | not_run |
| Q09 | not_run |
| Q10 | not_run |
| Q11 | not_run |
| Q12 | not_run |
| Q13 | not_run |
| Q14 | not_run |
| Q15 | not_run |
| Q16 | not_run |
| Q17 | not_run |
| Q18 | not_run |
| Q19 | not_run |
| Q20 | not_run |

Correct: 0; not run: 20. NULL, truncated and refused flags are unknown for
every question. These are not 20 wrong answers or 20 successful answers.

## Cleanup and verification

Raw evidence: `evidence/g-s5d-mysql-20261105/`.
Validation: `evidence/g-s5d-validation-20261105/`.

- Pre-run lane-g container and volume scans were empty.
- One target and one source were used, serially within this single run;
  no other database run was started.
- Both cleanup receipts contain `failures: []`; result records
  `containers_left: 0`. Independent post-run lane-g scans are empty.
- Pre/post searches found no stale `T6-*.patch` files, so none were deleted.
- All 254 files in the run manifest were SHA-256 verified.
- `cargo check --workspace --locked`: passed.
- `PYTHONDONTWRITEBYTECODE=1 python3 -m unittest discover -s tools/oo-test -v`:
  69 passed.
- `python3 specs/scripts/check_spec.py`, `check_plan.py`, and
  `check_v02_plan.py`: passed.

Run `SHA256SUMS` SHA-256:

```text
87ce5f907fd176d3b259e41136cea2601b5f0b8c3486435c3264db7129884a05
```

No shared-file patch, Cargo.lock change or Rust edit; Rust formatting is
not applicable. Follow-up requires building the harness's local executable
prerequisites and a separately authorized fresh run. This report does not
claim T6 acceptance, full-fixture T5, other engine/version coverage, T7,
public deployment or business acceptance.
