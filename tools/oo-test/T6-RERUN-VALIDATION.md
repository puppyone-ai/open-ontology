# G-S5b: shared fixes exercised; owner-review composite-key blocker

STATUS: blocked. S5-G-01 is **not done**. No T5/T6 acceptance is claimed.
The branch was rebased on `v0.2/product-loops` (already up to date).
Harness change: `d2c3ef068`; starting branch commit: `4f1ee982c`.

## Delivered

- Removed the superseded numeric and Builder follow-up patch files.
- The retained Builder's own structural preflight over the sealed inventory
  supplies the batch count. The harness prints and records three calls per
  planned batch: initial attempt plus two repairs. Values outside 1..4096
  fail before model dispatch. Other retained-cycle callers retain six calls.
- The separately calculated preflight exactly matches the Builder's actual
  preflight on all three engines. No alternative batching formula is used.
- The default reviewed route remains intact: real Builder, accept-all owner
  review, then publisher. No fence shortcut, fabricated scalar key, removed
  entity, changed gold value, or model retry was used to bypass the blocker.

## Executed runs

Each command used the default 2,000,000-event fixture and 200 small tables.
The retained build covers the 12 gold tables, not full-fixture T5.
Model endpoint/key were resolved through the authorized private secret_refs.

```sh
tools/oo-test/oo-test t6 --engine postgres --run-id g-s5b-pg-reviewed-20261104 --execute
tools/oo-test/oo-test t6 --engine mysql --run-id g-s5b-mysql-reviewed-20261104 --execute
tools/oo-test/oo-test t6 --engine oracle --run-id g-s5b-oracle-reviewed-20261104 --execute
```

| Engine / measured version | Planned batches | Builder limit | Actual Builder calls | Agent calls | Exit |
|---|---:|---:|---:|---:|---|
| PostgreSQL 17.11 | 7 | 21 | 11 | 0 | 1, owner review refused |
| MySQL 8.4.7 | 7 | 21 | 11 | 0 | 1, owner review refused |
| Oracle 23.5.0.24.07 | 13 | 39 | 13 | 0 | 1, owner review refused |

All three Builder results reach `review_required`, with zero unresolved
items. Total real-model calls: 35, all Builder calls. The combined
Builder/Agent dispatch bounds printed before Builder dispatch are 181,
181 and 199. Currency cost remains unmeasured.

All 20 source-computed gold answers agree across the engines. This is
source-oracle agreement only: no share-prompt binding or Agent SQL happened.
There are zero correct, wrong, refused or truncated **observed answers**;
NULL status is unknown, not false or zero. Every question is explicitly
`not_run` in each run's `result.json`:

| Question | PostgreSQL | MySQL | Oracle |
|---|---|---|---|
| Q01 | not_run | not_run | not_run |
| Q02 | not_run | not_run | not_run |
| Q03 | not_run | not_run | not_run |
| Q04 | not_run | not_run | not_run |
| Q05 | not_run | not_run | not_run |
| Q06 | not_run | not_run | not_run |
| Q07 | not_run | not_run | not_run |
| Q08 | not_run | not_run | not_run |
| Q09 | not_run | not_run | not_run |
| Q10 | not_run | not_run | not_run |
| Q11 | not_run | not_run | not_run |
| Q12 | not_run | not_run | not_run |
| Q13 | not_run | not_run | not_run |
| Q14 | not_run | not_run | not_run |
| Q15 | not_run | not_run | not_run |
| Q16 | not_run | not_run | not_run |
| Q17 | not_run | not_run | not_run |
| Q18 | not_run | not_run | not_run |
| Q19 | not_run | not_run | not_run |
| Q20 | not_run | not_run | not_run |

## Owning component and root cause

**Native owner review / reviewed-community compiler / OOS entity contract**
(lane A and coordinator integration), rather than Gateway query execution.

1. The retained Builder creates an order-line entity whose declared primary
   key is `(order_id, line_no)`. Oracle carries the same pair under native
   encoded identifiers. See `builder-baseline/review-input.json` and
   `builder-baseline/facts.json` in each run.
2. `crates/oo-cli/src/admin/review.rs` `items()` supplies a default entity key
   only for a single-column primary key. A composite key becomes JSON null.
   `prepare()` then requires nonempty scalar text for key/display and emits
   `REVIEW_INPUT_SHAPE`. Each run's `builder-baseline/review.log` records it.
3. This is not merely an error-message issue:
   `crates/oo-builder/src/reviewed_community.rs` `EntitySelection.key` is
   `String`, and `crates/oo-core/src/oos/model.rs` `EntityFile.key` is one
   `TableName`. An accept-all composite identity has no representation in
   that path. The publisher is never invoked and Gateway is never started.
4. The retained observations contain 240 order-line rows, with distinct
   counts 120 for order_id and 2 each for line_no and qty. No available
   scalar column is a valid substitute for the composite entity identity.

`g-s5b-review-diagnostic-20261104` independently replays the native command
against the unchanged real input and decision files for each engine:

```sh
target/debug/oo review \
  --input evidence/RUN/builder-baseline/review-input.json \
  --review-file evidence/RUN/builder-baseline/decisions.json \
  --out evidence/DIAGNOSTIC/NEW-reviewed.json
```

All three replays exit 1 with `REVIEW_INPUT_SHAPE`. A separate synthetic
single-key control reaches `reviewed`; a separate composite-key control
reproduces the refusal. These controls use the existing replay model,
not paid calls or altered acceptance inputs. The diagnostic records input
and binary hashes plus product source blob equality to branch base
`4f1ee982c`; no separate base checkout execution is claimed.

No shared-file fix is proposed or applied. A correct fix requires a
coordinated entity-identity contract change, or an explicit reviewed
representation preserving composite identity. Silently taking the first
key, removing the entity, or bypassing review would not satisfy this card.
The numeric fix passes its focused tests, but this run cannot establish
its Gateway login outcome because review blocks the next stages.

## Validation and evidence

`evidence/g-s5b-validation-20261104/`:

```text
cargo check --workspace --locked                         PASS
python3 -m unittest discover -s tools/oo-test -v          68 passed
cargo test --locked -p oo-pipeline --lib engine_load_pg   2 passed
check_spec.py / check_plan.py / check_v02_plan.py         PASS
Native CLI, Builder, Gateway locked builds               PASS
Standalone oo-load and release-coordinator locked builds PASS
retained_gateway_install (owned DB, each engine run)      PASS
```

No Rust source or Cargo.lock changed; no Rust formatting was needed.
No unrelated test failure was waived. Evidence manifests were checked
against every listed file. SHA-256 of each `SHA256SUMS`:

```text
5a0efe503796da021eead43c7906dec7dcac613f91230825c5be4775dbbcae5d  g-s5b-pg-reviewed-20261104
0ac77e1487c06634c6ba26b4915057b856f8636785c1e15a3c699907d6b41f3f  g-s5b-mysql-reviewed-20261104
ec95e1891d5dd1ffaee185e9853ddfcf1b3158795419d4e6368322ba336eee27  g-s5b-oracle-reviewed-20261104
59617620703c776f8da2690f53aa069a0e379e55145d6420244e0dac5e33f7a0  g-s5b-review-diagnostic-20261104
```

Prior failed evidence is retained unchanged. No full-fixture T5, remaining
P0 versions, source-audit T7, publication, Agent dialogue, public deployment
or business acceptance follows from these results. After cleanup, both
container and volume scans filtered by `oo.lane=g` are empty. No unrelated
resources were stopped or removed.
