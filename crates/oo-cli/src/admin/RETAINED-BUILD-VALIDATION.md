# A3-S4b retained build acceptance

## Final acceptance

Status: complete for card A3-S4b, S4-A-02. The first-pass failures below are
preserved, not reclassified as passes.

The lane was rebased again onto integration `412c522c1`, which includes the
coordinator-owned Loader fix preserving decimal precision and uint64 typmods.
Fresh fixtures were then run through the committed harness directly:

```sh
python3 crates/oo-cli/src/admin/test_retained_live.py --run-id a3-s4b-pg-12 --engine postgres
python3 crates/oo-cli/src/admin/test_retained_live.py --run-id a3-s4b-mysql-02 --engine mysql
python3 crates/oo-cli/src/admin/test_retained_live.py --run-id a3-s4b-oracle-04 --engine oracle
```

| Engine | Result | Model calls | Acceptance SHA-256 |
| --- | --- | --- | --- |
| PostgreSQL 17.11 | Passed | 3 initial, 0 data-only | `a65a1aeb39d241e3c0572332fba09f247643149004dd508ce45d49fb6e2c80ba` |
| MySQL 8.4.7 | Passed | 3 initial | `87daff0819088a90476a8de7dc927598efeb3a5a60b640b625a904d667e12f41` |
| Oracle 23.5.0.24.07 | Passed | 3 initial | `0f4fc1d29157e49230c38a84ba160b3dd9fdb31ffea60d66e7da0bd3768fbec6` |

Each run starts empty and SIGKILLs the actual CLI after each of the eight
durable stage checkpoints: load, candidate, Builder, self-review, publication,
Gateway, first token, share prompt. Each next command uses `--resume`; model
work and credentials are not reissued. The debug-only
`OO_BUILD_TEST_PAUSE_AFTER` failpoint stops at the checkpoint before SIGKILL;
release builds have no failpoint. The harness still checks that non-TTY review
without a decision file blocks.

All three runs then execute the printed login command, `oo whoami`, `oo tables`
and a native Gateway query. Complete-build replay returns the same prompt and
state. PostgreSQL changes one source table, completes the unchanged table's cut
via snapshot resume, uses the reviewed registry with zero model calls, and
serves `1001.1250` through the native CLI under the new publication. No model
usage/call-count file exists for that data-only Builder cycle.

`cargo check --workspace --locked` and `cargo test -p oo-cli --locked` pass
(73 tests passed, 2 ignored); format and three spec/plan checks pass.
Build/test evidence: `a3-s4b-integrated-01` and `a3-s4b-final-01`.
All A3 containers and volumes are removed.

No shared-file patch or tracked Cargo.lock change remains requested. Only
`crates/oo-cli/src/admin/` is changed by this lane.

Limits remain explicit: stage-boundary recovery is tested, not arbitrary
in-flight model or enrollment-response recovery. Incomplete Builder output and
ambiguous enrollment responses require reconciliation. The unchanged-cut
fallback adds bounded reads/copies. Source time remains unmeasured and
cross-table consistency is not claimed.

## First-pass report (superseded)

Status at this point: partial; S4-A-02 was not accepted in full.

The branch was rebased onto `v0.2/product-loops` at `5717b8f4c`. No shared
source, migration, root manifest, or tracked lockfile was changed.

## First-pass checks

- `cargo fmt -p oo-cli`
- `cargo check --workspace --locked`: passed.
- `cargo test -p oo-cli --locked`: 73 passed, 2 ignored.
- Evidence: `a3-s4b-resume-02` and `a3-s4b-cycle-01`.
- Dev/test debug info and incremental compilation were disabled for disk budget.
- Six pre-token SIGKILL tests exercise persisted checkpoint validation and
  tamper refusal. Existing tail/first-token tests exercise receipt replay and
  ambiguous dispatch refusal. These are not a live crash sweep of every
  external call; incomplete Builder output and lost enrollment responses still
  require reconciliation. No model-budget reset or automatic owner approval
  was added.

## First-pass live engines

Reproduction entry point:

```sh
python3 crates/oo-cli/src/admin/test_retained_live.py \
  --run-id UNIQUE --engine postgres
```

Use `mysql` or `oracle` for the other engines. The script requires the installed
`oo`, `oo-load`, `oo-gateway`, `oo-builder`, `selfbuild-budget`, and
`release-coordinator` binaries in `target/debug`, Docker, and the card-authorized
model secret references. Fixture setup is admin-only; the product's source
connection uses a read-only reader. All resources have `oo.lane=a3`.

The runs below used an evidence-local wrapper which additionally invokes a
read-only diagnostic against the public Gateway catalog loader on query failure.
The diagnostic does not change product source, credentials, grants, or data.
The fixtures are lane H's neutral `customers` and `orders`, 120-row scale,
12 columns total, not a one-column fixture. Fixture owner review uses explicit
hash-bound decision files; it is not evidence of human review.

| Engine | Run | Build / prompt / replay | Consumer path | Data-only cycle |
| --- | --- | --- | --- | --- |
| PostgreSQL 17.11 | `a3-s4b-pg-11` | Passed, 3 model calls | Enrollment saves credential; login table verification fails; explicit native query and direct mTLS query return 409 | Build completes, zero model calls, unchanged cut completed by snapshot resume; serving new data fails |
| MySQL 8.4.7 | `a3-s4b-mysql-01` | Passed, 3 model calls | Same query/catalog failure as PostgreSQL | Not requested/run |
| Oracle 23.5.0.24.07 | `a3-s4b-oracle-03` | Passed, 3 model calls | Printed login, whoami, tables and native query all pass | Not requested/run |

PostgreSQL and MySQL return `PUBLICATION_DRIFT`; the read-only catalog probe
reports `UnsupportedType`. Retained `orders.amount` is unconstrained `numeric`,
whereas Gateway admission requires a declared numeric precision. This is not
an mTLS failure. Oracle's lossless numeric-as-text projection is admitted.
No failed query is counted as a successful data answer. All three final runs
removed their containers; zero A3 containers remained.

Earlier failures are preserved: `a3-s4b-pg-07` lacked fixture token-admin
CONNECT, `a3-s4b-pg-08` lacked its advisory-lock EXECUTE, `a3-s4b-pg-09`
exposed the numeric refusal, and `a3-s4b-pg-10` also exposed unchanged-table
C5 candidate refusal. `a3-s4b-oracle-01` had a SQL*Plus fixture separator
error; `a3-s4b-oracle-02` failed on a transient Docker socket refusal. None
of those runs is a pass.

The CLI now handles the exact `publication_unverified` cycle refusal by
snapshot-resuming the same run ID before one candidate retry. Already completed
C5 tables are skipped; unchanged tables incur bounded source reads/copies.
This is not a zero-read cycle or cross-table consistent snapshot. The PostgreSQL
cycle has no usage log or call-count file and reports `model_calls=0`.

## Superseded shared-file proposal

The integration Loader fix supersedes this proposal. It is retained as the
historical handoff, not an outstanding patch request.

Proposed exact Loader patch, **not applied or validated** in this lane. Preserve
Arrow's precision/scale in retained storage instead of weakening Gateway's type
guard. The coordinator should add decimal/uint64 target tests, run the pipeline
tests, and rerun PostgreSQL/MySQL from fresh storage generations. Do not mutate
sealed historical cuts or reuse their untyped staging tables.

```diff
--- a/crates/oo-pipeline/src/engine_load_pg.rs
+++ b/crates/oo-pipeline/src/engine_load_pg.rs
@@ -216,6 +216,17 @@
                 return Err("load_schema");
             }
             let (oid, sql) = column(f)?;
+            let sql = match f.arrow_type.as_str() {
+                "uint64" => "numeric(20,0)".to_owned(),
+                "decimal128" => {
+                    let precision = f.precision.filter(|p| (1..=38).contains(p))
+                        .ok_or("load_decimal_precision")?;
+                    let scale = f.scale.filter(|s| (0..=precision).contains(s))
+                        .ok_or("load_decimal_scale")?;
+                    format!("numeric({precision},{scale})")
+                }
+                _ => sql.to_owned(),
+            };
             types.push(oid);
             definitions.push(format!(
                 "{} {sql}{}",
```

No tracked Cargo.lock changes are requested. Incomplete Builder restart and
enrollment response reconciliation remain separate protocol follow-ups.

## Historical evidence hashes

SHA-256 of the engine acceptance summaries (raw files remain git-ignored):

| Run | `acceptance.json` |
| --- | --- |
| `a3-s4b-pg-11` | `f18ff9e150b42b756abec71a3df5b47f0e67b854ec8c8947303d6793c17979f2` |
| `a3-s4b-mysql-01` | `84347c32496495e6153bfc033f87964edebb5e746c26997a21151311c96e192b` |
| `a3-s4b-oracle-03` | `5044ab65d657d0076a863815aa795d3db64ef8f067bf7e3ca0088771a4d66b82` |
