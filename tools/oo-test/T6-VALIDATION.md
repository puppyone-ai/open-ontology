# S5-G-01: harness delivered, acceptance blocked

Status: partial. No engine/version receives T6 or T5 acceptance from this work.
Work started on integration `6080fbb2f` and rebased onto `3ee1ec34e` after
the retained publisher landed. Only `tools/oo-test/` is changed.

## Implemented and checked

- `oo-test t6` owns labelled fixtures, loads H's neutral seed, computes all
  20 source gold answers through reader accounts and loads the 12 gold tables.
- A real-model Agent receives native share-prompt enrollment, binds through
  native login/whoami, and has only constrained native CLI tools. Source gold,
  source connections, shell and file tools are not exposed to it.
- Exact ordered answer comparison preserves NULL and distinguishes refusal,
  truncation, invalid/unverified answers, failures and questions not run.
- Native mapped names/types are authoritative, including Oracle identifiers.
  No Python implementation of the identifier encoder is introduced.
- Default publication now invokes the real retained Builder, native accept-all
  owner review and `publish-retained`; direct test fences require
  `--publisher fixture`.
- Credentials remain in owner-private secret-ref files. Enrollment material
  is redacted; tokens are passed by file or stdin. Cleanup checks lane/run
  ownership. Final Docker container and volume scans found no lane-G resources.

Validation evidence is under `evidence/g-s5-build-20261104/`:

```text
cargo check --workspace --locked                         PASS
python3 -m unittest discover -s tools/oo-test -v          67 passed
python3 specs/scripts/check_spec.py                      PASS (document integrity)
python3 specs/scripts/check_plan.py                      PASS (plan integrity)
python3 specs/scripts/check_v02_plan.py                   PASS (plan integrity)
```

Native CLI/Builder/Gateway and standalone oo-load/release-coordinator builds
passed with locked dependencies. No Rust source, migration or Cargo.lock was
changed. No unrelated failing test was waived or represented as passing.

## Real runs

All rows below use 2,000,000 events, 200 small seed tables, and the same
20-question source gold. The six gold files match exactly across engines;
that is source-oracle agreement, not Agent acceptance.

| Engine/version | Run ID | Mode | Result |
|---|---|---|---|
| PostgreSQL 17.11 | `g-s5-pg-final-20261104` | Fixture fences | Ready publication; one real Agent binding call; native login read verification fails; 20 questions not run |
| MySQL 8.4.7 | `g-s5-mysql-first-20261104` | Fixture fences | Same failure; one Agent binding call; 20 questions not run |
| Oracle 23.5.0.24.07 | `g-s5-oracle-third-20261104` | Fixture fences | Binding succeeds; 64 Agent calls; 5 correct, 1 refused, 14 budget failures |
| PostgreSQL 17.11 | `g-s5-pg-reviewed-20261104` | Real Builder/publisher path | Builder stops at six calls; 20 questions not run |
| MySQL 8.4.7 | `g-s5-mysql-reviewed-20261104` | Real Builder/publisher path | Builder stops at six calls; 20 questions not run |
| Oracle 23.5.0.24.07 | `g-s5-oracle-reviewed-20261104` | Real Builder/publisher path | Builder stops at six calls; 20 questions not run |

Reproduction entry, substituting the engine and a new run ID:

```sh
tools/oo-test/oo-test t6 --engine postgres --run-id NEW-RUN --execute
```

The initial fixture runs preceded the real publisher integration. Their
successful publication is explicitly the test-fence path, not production
publisher acceptance. Oracle's correct answers were Q01/Q02/Q03/Q07/Q09;
Q08 was refused. All other questions failed the historical Agent budget.
Native describe responses were about 27 KB each. The historical 60 KB
request/2M reservation limits were insufficient for multi-table questions.
Current limits are 250 KB/request, 12M reservations, 12 turns/question,
160 total Agent calls and a one-hour dispatch deadline. This budget adjustment
has unit coverage, but no successful 20-question real rerun is claimed.

Historical PostgreSQL/MySQL reviewed result files use `model_calls` for the
Agent only (zero). Their Builder `model-call-count.json` and usage receipts
record six calls each. Current output separates Agent/Builder counts and
reports the total; Oracle's reviewed result records 6 Builder + 0 Agent.
Original failed evidence has not been rewritten.

## Confirmed shared blockers

1. **Retained Builder's six-call ceiling.** PostgreSQL/MySQL preflight plans
   seven batches; Oracle plans thirteen. Every real-path run ends with
   `{"published":false,"reason":"retained_model_call_budget","stage":"failed"}`
   after six Builder calls. Review, publisher activation and Agent dialogue
   are therefore not reached. This is not a publisher success.

2. **Unconstrained target numeric.** The unchanged `engine_load_pg::column`
   projects decimal128/uint64 to unconstrained `numeric`. The Gateway requires
   bounded numeric precision. PostgreSQL/MySQL fixture publications therefore
   fail the login `/v1/tables` check with `409 PUBLICATION_DRIFT`.
   `g-s5-catalog-diagnostic-20261104` calls the real base Gateway catalog loader
   on the owned publication and records `catalog_error=UnsupportedType` and
   `unconstrained_numeric_grantable=false`. Target metadata records the actual
   unconstrained amount column. No widening of Gateway admission was attempted.

The shared implementations are unchanged from the corresponding integration
base; the live failures and catalog probe use those real compiled functions,
not a replacement policy or a mocked failure.

## Requested exact patches

- `T6-NUMERIC-FOLLOWUP.patch`: preserve decimal precision/scale and bound uint64
  to numeric(20,0) in newly created target generations; includes focused Rust
  tests and the existing test's return-type adjustment.
- `T6-BUILDER-FOLLOWUP.patch`: permit an explicit bounded 32-call Builder run,
  keep other retained-cycle callers at six by default, and align this harness's
  preflight/documented total at 192 calls (32 Builder + 160 Agent).

Both are **unapplied proposals**, not changes to shared files. Both pass
`git apply --check`; the numeric proposal is rustfmt-formatted. Neither has
been built or run as an applied product change. Coordinator review and
crate/live regression tests are required. The numeric fix requires a fresh
load/generation; do not mutate the sealed failed publication.

## Evidence bindings

SHA-256 of each run's `SHA256SUMS`; every listed file hash was rechecked:

```text
8311299124dda441dac593f5121ebef73a10379f1256b7a3066f55b2cceae5a9  g-s5-pg-final-20261104
1e3477132b3a8d81cdf9b01126c708cece66f0438a6f3b6310547c51ffd3b751  g-s5-mysql-first-20261104
4a81e31cb7891a8e3c703f937919606d2d18d7bd6e67ef784a8d99c67b6dcced  g-s5-oracle-third-20261104
a14d95e1ce8750d2f3f954ad12775f119d42974aa26cbd4be0266bb1dc1b35fc  g-s5-pg-reviewed-20261104
912fa7b58aee4558eec8dbf25b328196de73ac97b732c2db526ca32fc88a9550  g-s5-mysql-reviewed-20261104
d20a5319030574163b4ba0c51f00957d0c4a7af3b876a3aada5d22c3e9579c5e  g-s5-oracle-reviewed-20261104
029b16c05361fbfc2ecaec046a25167dd4051a6444ed70994598b75e13facdb8  g-s5-catalog-diagnostic-20261104
```

Other exploratory failed runs remain under their original ignored run IDs.
No full-fixture T5, all-P0-version coverage, T7 source audit, public deployment
or business acceptance is established. S5-G-01 remains **not done**.
