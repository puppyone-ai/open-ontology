# H-S5d: corrected full-scale P0 T0/T1/T8 acceptance

2026-11-06 (Asia/Shanghai). Card complete; S5-H-01 card exits pass.
This does not close the broader T7 work owned by lane T.
Rebased onto `590cf39f7` before work. Product fixes for keyless whole-table
C5 and streamed Oracle LOB digests were already integrated.

## Execution and verdicts

For each version below, sequentially:

```sh
OO_TEST_LANE=h OO_TEST_ORACLE_WORKER_IMAGE=<pinned-local-image-id> \
  tools/oo-test/oo-test matrix --engine ENGINE --versions VERSION \
  --suites T0,T1,T8 --hook T0=tools/oo-test/t0.py --hook T1=tools/oo-test/t1.py
```

Defaults were unchanged: 2,000,000 events, 200 small tables, plus all type,
keyless, growth and 300-column fixtures. Every final command exits 0.
T1 compares the concurrent snapshot after one C5 correction, not a replacement
full load. Replay causes no rewrite; digest-only and periodic-full-read keyless
idle cycles produce zero writes and zero new generations. Consistency is B.
T0 uses a synthetic model replay, not live-model acceptance. T8 measures sampled
reader peaks and time-weighted duty, not a continuous upper bound.

| Engine | Measured version | T0/T1/T8 | Compared tables | Reader peak | Max duty % | Evidence |
| --- | --- | --- | ---: | ---: | ---: | --- |
| mysql | 5.7.44 | pass/pass/pass | 231 | 1 | 15.804 | R01 |
| mysql | 8.0.44 | pass/pass/pass | 231 | 1 | 12.133 | R02 |
| mysql | 8.4.7 | pass/pass/pass | 231 | 1 | 19.293 | R03 |
| mysql | 9.5.0 | pass/pass/pass | 231 | 1 | 21.662 | R04 |
| postgres | 13.23 | pass/pass/pass | 233 | 2 | 7.953 | R05 |
| postgres | 14.24 | pass/pass/pass | 233 | 2 | 6.863 | R06 |
| postgres | 15.19 | pass/pass/pass | 233 | 2 | 7.784 | R07 |
| postgres | 16.15 | pass/pass/pass | 233 | 2 | 10.544 | R08 |
| postgres | 17.11 | pass/pass/pass | 233 | 2 | 8.618 | R09 |
| postgres | 18.6 | pass/pass/pass | 233 | 2 | 7.698 | R10 |
| oracle | 23.5.0.24.07 | pass/pass/pass | 231 | 1 | 11.230 | R11 |
| oracle | 21.3.0.0.0 | pass/pass/pass | 231 | 1 | 14.714 | R12 |
| oracle | 11.2.0.2.0 | pass/pass/pass | 231 | 1 | 13.849 | R13 |
| oracle | 19.19.0.0.0 | pass/pass/pass | 231 | 1 | 12.257 | R14 |

MySQL 5.7 and Oracle 11g/21c ran under linux/amd64 emulation; the other
versions ran natively. All sampled duty values meet the 25% limit.

### Run IDs

- R01: `h-s5-20261105T160228697399Z-2bc024bc-9c34-4033-b5e7-c05cf41641c5`
- R02: `h-s5-20261105T164616720201Z-0c45bd26-5dc5-4b29-a84c-e865306a1823`
- R03: `h-s5-20261105T182602272699Z-8b08c893-8c2d-4a02-bb09-5fa9c102bc4d`
- R04: `h-s5-20261105T194425447258Z-52104c4b-23b6-4f90-a90b-a0d7feabc5c9`
- R05: `h-s5-20261105T205925356342Z-8ce4bd83-d25e-413a-acda-d5c733199cd2`
- R06: `h-s5-20261105T213250174882Z-d117f965-8f86-45ed-97f0-9e292be3d750`
- R07: `h-s5-20261105T221043529040Z-014e0023-0688-4d86-b584-cf85bb720b57`
- R08: `h-s5-20261105T224202397066Z-e85bc071-5a4b-423c-b5b7-be8eb5e369ee`
- R09: `h-s5-20261105T231609385802Z-87cc3b2f-55fc-4f7d-8104-e310d80266ca`
- R10: `h-s5-20261105T234735708177Z-20b475b2-506f-4e8d-a20d-0be5f03052b9`
- R11: `h-s5-20261106T001742387422Z-f7686181-9d11-45ba-bb2a-ce524e982a24`
- R12: `h-s5-20261106T003149530632Z-b339d2c5-8f4a-4180-8dc2-3c6f04687716`
- R13: `h-s5-20261106T005940496965Z-4e88876a-1170-499c-819d-59d8a64a8c4a`
- R14: `h-s5-20261106T012508102396Z-b7e75dc4-ee1c-4e45-81f4-3cee0c108455`

## Retained failure and harness changes

- `e2210d9cf`: make owned MySQL reader configuration repeatable for combined
  T0/T1; retain SSL and SELECT/SHOW VIEW grants. Honor explicit lane H for
  Oracle 19c instead of the historical E19 default; ownership checks agree.
- `986d260dd`: extend the outer hook deadline from one to four hours. The
  loader's two-hour per-operation bound and all verdicts remain unchanged.
- Initial MySQL 8.4 attempt `h-s5-20261105T172302625157Z-f7380d93-5429-4097-b7e4-179864b6e0c8` timed out at the
  outer one-hour deadline. T0 passed, T1 failed, T8 had no completed verdict;
  both latter suites remain explicit failures. The fresh retry passed.
  The killed hook left its Agent DB; its exact lane/run identity was checked
  before manual removal. No unrelated resource was removed.
- MySQL 5.7/8.0/8.4 used the debug loader. Starting with MySQL 9.5, the same
  source was built with `cargo build --release --locked --manifest-path
  tools/oo-load/Cargo.toml` and installed at the hook's local binary path
  between targets. Both hashes and the switch boundary are retained in
  `evidence/h-s5d-20261105/release-binary-switch.json`.
- After PG16, an unrelated invocation had two containers labelled H, with
  a different run ID. The guard stopped; our PG16 cleanup was verified and
  continuation checks were scoped to our exact run ID. Foreign resources
  were neither stopped nor removed. `foreign-h-label.json` records this.

## Index, matrix and checks

Raw orchestration and validation: `evidence/h-s5d-20261105/`.
All 15 attempts were merged with `oo-test matrix --merge-only` and repeated
`--merge-evidence` arguments, validating each raw result hash. Merge run:
`h-merged-909ae996-3d16-4682-87d5-42b584bba8ae`.
Its exit 1 is expected: it preserves the two historical timeout failure rows.
88 new rows (86 pass, 2 fail) were appended to the existing 727, without
rewriting historical observations. There are 815 rows total. The latest
observation for each requested version/suite passes.

The authorized index and regenerated matrix are committed directly; no
coordinator patch or Cargo.lock change is needed.

- `python3 -m unittest discover -s tools/oo-test -p 'test_*.py'`: 134 pass.
- `cargo check --workspace --locked`: pass before each commit.
- CLI, loader, MySQL doctor builds with `--locked`: pass; optimized loader
  build also passes. No Rust files changed, so touched-crate formatting and
  crate unit tests are not applicable.
- `check_spec.py`, `check_plan.py`, `check_v02_plan.py`: pass.
- `gen_support_matrix.py --check --self-test`: pass, 815 rows and 9 cases.
- No pre-existing-failure exemption was used. Initial Python regression run
  exposed a test's ambient lane dependency; the test now explicitly selects H.

No product engine, audit/T7, fixture gold, root manifest, lockfile, migration,
token file, customer system or other worktree was modified. At most two owned
database containers ran at once (Oracle 11g also used a non-database worker).
Final exact-run inventory: no owned containers or volumes remain, across all
15 attempts (`final-cleanup.json`). Foreign containers remain untouched.

Not tested here: T2-T7, live models, public Gateway, business acceptance or
three consecutive nightly runs. Existing matrix results for those capabilities
are not new acceptance from this card.
