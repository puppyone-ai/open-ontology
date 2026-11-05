# H-S5b: D17 Oracle reruns and product T0

2026-11-05. Status: partial at plan level. The available-version card work
passes; Oracle 19c cannot run without its licensed image. S5-H-01 is not
closed (T7 belongs to lane T). S5-E-04 is not closed by these T0/T1/T8 runs;
the earlier Oracle 23 T3 retention gap and 19c gate remain.

Rebase onto `v0.2/product-loops` was already current at
`18c1bd3c9874c646628c7fe73e97ad8ad849d270`.
No audit/T7, engine, fixture gold, shared document/index, migration,
Cargo.lock, scheduler, token store, other worktree or customer system changed.

## Changes and retained failures

- D17 had reached `t1.py`'s scalar comparator but not `oracle_t1.py`'s LOB
  path. TYPE_EDGES includes LOBs, so the initial 23.5/21.3 reruns still
  rejected equal instants with different offsets. The LOB path now uses UTC
  instants too. Regression: fails before the fix, passes after; changed
  instants, naive timestamps, ORIGINAL_ZONE, empty LOB/NULL and submicrosecond
  precision retain their strict behavior. No gold value changed.
- `t0.py` invokes native `oo doctor --json`, not a substitute probe. It
  checks measured version, engine capabilities/settings, missing-privilege
  advice, chunk/byte-estimate fields, and status/exit agreement for auto,
  C5 and C1. Doctor plans events/order_lines on the full seeded fixture:
  this is not all-table planning coverage. Fixture C1 is correctly blocked
  with DBA advice, not accepted as usable CDC.
- The model is a local synthetic replay; secrets are runtime-generated in
  an isolated HOME and deleted. Only the fixture admin configures TLS and
  FLASHBACK. Oracle 11g uses the existing content-pinned thick-worker image.
  A private exact-argv shim only relabels/names the product worker for lane H;
  it does not replace the doctor, transport protocol or capability results.
- Initial 11g T0: auto passed, C5 returned `budget fact=call_timeout`;
  C1 did not run. That remains a failure. Worker names are now unique for
  each mode, avoiding a potential Docker --rm name-reuse race; the timeout's
  exact cause was not independently established. All three fresh modes pass
  their expected diagnostic outcomes. Initial 11g T1/T8 already passed after
  the D17 LOB fix; a fresh full run of all three versions confirms it.
- Initial and final attempts are both retained. The initial run spans the
  comparator repair; only the final Oracle run is used for final acceptance.

## Runtime results

All fixtures use the defaults: 2,000,000 events, 200 small tables, 300-column
wide table. Oracle T1 compares all 231 tables after concurrent writes and
quiescent correction, then verifies resume causes no target rewrite.
Consistency is B, not a transactional cross-table snapshot claim.

| Engine | Measured versions | Product T0 | Fresh T1/T8 |
|---|---|---|---|
| MySQL | 5.7.44, 8.0.44, 8.4.7, 9.5.0 | pass each | not rerun on this card |
| PostgreSQL | 13.23, 14.24, 15.19, 16.15, 17.11, 18.6 | pass each | not rerun on this card |
| Oracle | 23.5.0.24.07 | pass | pass/pass, 231 tables |
| Oracle | 21.3.0.0.0 | pass | pass/pass, 231 tables |
| Oracle | 11.2.0.2.0 | pass | pass/pass, 231 tables |
| Oracle | 19c, unmeasured | blocked: no locked image | not run |

MySQL 5.7 and Oracle 11g/21c are emulated linux/amd64; the rest are native.
For every passing T0, auto/C5 return ok/exit 0 and C1 returns blocked/exit 1
with privilege advice. Missing setup is tested, not performed by the product.
T0 is diagnostic acceptance; no actual C1 workload is implied.

| Oracle version | T8 sampled reader peak | Maximum load duty | Logical target bytes |
|---|---|---|---|
| 23.5.0.24.07 | 1 | 4.138% | 194001282 |
| 21.3.0.0.0 | 1 | 11.125% | 191991881 |
| 11.2.0.2.0 | 1 | 6.406% | 193091517 |

All three loads per version meet the 25% duty and one-reader topology
budgets; these are sampled measurements, not continuous upper bounds.
Model tokens for T1 are zero. No target-byte cap is configured.

## Commands and raw evidence

Raw logs, build/test outputs and generation receipts:
`evidence/h-s5b-20261105/`.

Builds, all passed:

```sh
CARGO_INCREMENTAL=0 CARGO_PROFILE_DEV_DEBUG=0 cargo check --workspace --locked
CARGO_INCREMENTAL=0 CARGO_PROFILE_DEV_DEBUG=0 cargo build --locked -p oo-cli
CARGO_INCREMENTAL=0 CARGO_PROFILE_DEV_DEBUG=0 CARGO_TARGET_DIR="$PWD/target" cargo build --locked --manifest-path tools/oo-load/Cargo.toml --features mysql-binlog
CARGO_INCREMENTAL=0 CARGO_PROFILE_DEV_DEBUG=0 CARGO_TARGET_DIR="$PWD/target" cargo build --locked --manifest-path deploy/lite/mysql-doctor/Cargo.toml
```

Runtime commands use `OO_TEST_LANE=h`. For Oracle, the worker environment
contains only the content ID resolved from the existing local worker image.

```sh
tools/oo-test/oo-test matrix --engine oracle --versions 23ai,21c,11g --suites T0,T1,T8 --hook T0=tools/oo-test/t0.py --hook T1=tools/oo-test/t1.py
tools/oo-test/oo-test matrix --engine mysql --versions 5.7,8.0,8.4,9.x --suites T0 --hook T0=tools/oo-test/t0.py
tools/oo-test/oo-test matrix --engine postgres --versions 13,14,15,16,17,18 --suites T0 --hook T0=tools/oo-test/t0.py
tools/oo-test/oo-test matrix --engine oracle --versions 19c --suites T0 --hook T0=tools/oo-test/t0.py
```

| Run ID | Command/result |
|---|---|
| h-s5-20261105T062214326715Z-77cb6781-f873-4e76-9b15-accebb5d3d21 | Initial Oracle, exit 1; failures above retained |
| h-s5-20261105T064855064565Z-10bff5f1-5390-4b70-826e-7fc9f7011030 | MySQL T0, exit 0 |
| h-s5-20261105T065256171331Z-a1bb67b6-0216-4a54-98e3-cc317c407fdd | PostgreSQL T0, exit 0 |
| h-s5b-oracle19c-blocked-20261105 | Oracle 19c, exit 1 before startup; original CLI traceback/exit retained and indexed as an unexecuted failure, not a doctor result |
| h-s5-20261105T065548932776Z-3ea0071c-cf6d-4945-8b1c-f69f40ccacd0 | Fresh Oracle T0/T1/T8, exit 0 |
| h-merged-36a8ca10-aafc-448b-a9ab-8def7231455d | Hash-validated merge of all five runs; exit 1 because earlier failures and 19c remain |

`oo-test matrix --merge-only --merge-evidence ...` merged 57 rows. The
candidate index appends these to the existing 565 rows (622 total).
`evidence/h-s5b-20261105/regenerate.py` records the exact merge command and
calls the unchanged `specs/scripts/gen_support_matrix.py` with only INDEX
and OUT redirected into ignored evidence. Generation and `--check --self-test`
pass: 622 rows, all seven derivation cases. No rules or prior rows changed.

## Regenerated version x capability table

Candidate output, not applied to coordinator-owned files. Separate unmeasured
image-version identifiers from old blocked runs remain designed.
T5/T6 capabilities are absent. No T7 pass is inferred.

| Target | Version | full | cdc | digest | schema_change |
|---|---|---|---|---|---|
| mysql57-selfmanaged | 5.7.44 | experimental | experimental | verified | verified |
| mysql8-selfmanaged | 8.0.44 | experimental | experimental | verified | verified |
| mysql8-selfmanaged | 8.4.7 | experimental | experimental | verified | verified |
| mysql9-selfmanaged | 9.5.0 | experimental | experimental | verified | verified |
| oracle-11g-xe | 11.2.0.2 | designed | designed | designed | designed |
| oracle-11g-xe | 11.2.0.2.0 | experimental | designed | verified | verified |
| oracle-19c-selfmanaged | 19c | designed | designed | designed | designed |
| oracle-21c-xe | 21.3.0 | designed | designed | designed | designed |
| oracle-21c-xe | 21.3.0.0.0 | experimental | designed | verified | verified |
| oracle-23ai-selfmanaged | 23.5 | designed | designed | designed | designed |
| oracle-23ai-selfmanaged | 23.5.0.24.07 | experimental | experimental | experimental | verified |
| postgres-selfmanaged | 13.23 | experimental | experimental | verified | verified |
| postgres-selfmanaged | 14.24 | experimental | experimental | verified | verified |
| postgres-selfmanaged | 15.19 | experimental | experimental | verified | verified |
| postgres-selfmanaged | 16.15 | experimental | experimental | verified | verified |
| postgres-selfmanaged | 17.11 | experimental | experimental | verified | verified |
| postgres-selfmanaged | 18.6 | experimental | experimental | verified | verified |

## Shared-file handoff and checksums

Exact requested diff: `evidence/h-s5b-20261105/shared-matrix.patch`.
It changes only `execution/v0.2/evidence-index.json` and
`docs/database-support/support-matrix.generated.json`.
`git apply --check` passes against this worktree's base. It is intentionally
not applied: these files are coordinator-owned. If other lane rows have
arrived, append these 57 rows and regenerate rather than overwrite them.
No Cargo.lock changes or other shared-file patches are requested.

| Artifact | SHA-256 |
|---|---|
| Exact shared patch | 9f368e52735064e20079cf4029311553698aecd179fdccb9fcb7b2ba396f6719 |
| Candidate index | 520cb4af62ee3bf495d689f8f8e45b30f419c0c5af149d393573650c2cb7427a |
| Candidate generated matrix | 27fa1e3060a2b8ee101f7a9ff068cfa2fb7026677b5345bbd1cdc36655aa0e6f |
| Final 23.5 T1/T8 report | 1c626f13b75a95a943b17a3fa787c15655ef49625d28f06b5cea7bce2baf211b |
| Final 21.3 T1/T8 report | 303bffee961cdd3970f5a313361b0b27ab9959a5274c651fabd8b2070b845622 |
| Final 11.2 T1/T8 report | a4f008801ed701e41e8f90b4a39bbdbb79cbaf767abedea3cc5858574d374a79 |

## Local validation and remaining exits

- `python3 -m unittest discover -s tools/oo-test -p 'test_*.py'`: 102 pass.
- `python3 -m unittest discover -s ci -p 'test_*.py'`: 39 pass.
- `check_spec.py`, `check_plan.py`, `check_v02_plan.py`: pass.
- `cargo check --workspace --locked`: pass before each commit.
- No Rust edits: touched-crate formatting/unit tests are not applicable.
- No pre-existing test failure exemption was used.
- The final unit rerun waited at the unchanged 20 GiB disk gate. Removing
  only 817856543 bytes of Rust metadata generated by this task let it
  complete (102 pass); binaries and all evidence were retained. The exact
  removed paths are in `own-build-metadata-cleanup.json`.
- Final lane-H container and volume inventories are empty
  (`final-cleanup.json`). At most two owned database containers ran at once;
  no unrelated resources or images were removed. Disk floors were unchanged.
- Coordinator follow-ups: ingest the exact patch/new rows, complete lane T's
  T7, resolve Oracle 19c licensing and the earlier Oracle 23 T3 retention
  case. T3/T4/T5/T6 and live-model acceptance were not rerun on this card.
