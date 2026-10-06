# a259 review of frozen integer quantity

Verdict: APPROVE_LIMITED for frozen commit `e447249ab1a7f4c82df4729090b1285c9ef4a4ec`.

This directory reviews that commit. It does not edit product source. Quantity stays an extension of the existing reviewed-policy adapter. It is not a new component.

`live/outcome.json` records `parent_acceptance` false, `component_approval` false, `development_unit` false, `m1_i01` `not_accepted`, and `m1_i02` `not_accepted`. This review does not accept a parent plan item, does not close M1-D14G, and does not release.

## What was reviewed

Contract `oo.gateway.reviewed-policy-quantity/v1` at `contracts/gateway/reviewed-policy-quantity-v1.md`. Git blob `827dd0b406ffccbdd2c82514631cda7b4257277e`.

Adapter cell `compatible()` in `crates/oo-gateway/src/reviewed_policy.rs`. Git blob `95e68417e6fe1be024123e3aa2b7c2301402eb21`. The archived integer arm, copied to `linux/run/integer-arm.txt`, admits `SemanticType::Quantity` beside identifier, foreign key, code, and status code. That arm does not admit `SemanticType::Money`.

CLI fixture `crates/oo-gateway/tests/reviewed_policy_quantity.rs`. Git blob `b31773e9cb61d73c88b217225aa1d5b6de140fa9`.

Archive `linux/run/fixed-base.txt` is `e447249ab1a7f4c82df4729090b1285c9ef4a4ec`. Builds used `git archive` of that commit. `Cargo.lock` file sha256 stayed `16e6afda29ae4bae251d569f3c4ad16459527f0d0323598d8e4e0971818d458a` (`linux/run/lock-hash.txt`). The overlay registry-triple diff is empty.

## Toolchain

Pinned image `docker.io/library/rust@sha256:0f24958fa693fe838859faf64a0ec78d382cd543a6b6efd0fa7a3bcb10ea90d5`, `linux/arm64`, `--pull never`. `rustc 1.94.0 (4a4ef493e 2026-03-02)`. The successful builder is 6442450944 bytes, `NanoCpus` 1000000000, `CapDrop` `["ALL"]`, `Privileged` false, network `oo-a259-eef831aa-net`, `OOMKilled` false. Container `65917e9f37317fec171f6a730cb306512d0e0434cada6bdad2325495c5ab9eef`.

Private PostgreSQL image `docker.io/library/postgres@sha256:3aac91d19d5b7aff4c650dddfacc1a26c436459aafd4eb51ab0bf1748e990dbb`. Container `oo-a259-pg`, alias `oo-a259-pg`, SAN `DNS:oo-a259-pg`. `pg/up.exit` is 0. `live/server.txt` records `ssl=true` and `server_version=17.10 (Debian 17.10-1.pgdg12+1)`.

Recipe origins are in `origin/recipes.txt`.

## Runs

| Run | Top exit | Docker exit | Container |
|---|---|---|---|
| `linux/run-attempt-1` | 101 | 101 | `1fa7017a54478e43aa2c2c5d6d49d3cb7d1045e649f0e82b8e51d7fc56c036f5` |
| `linux/run-attempt-2` | 101 | 101 | `d358b8159205e532a935843f7c6526817eba79dcbeddeec0203e7ec75d4b1480` |
| `linux/run-attempt-3` | 1 | 1 | `63b6078c44f9eb78371f72599e3ebd2c0e0916c14901e5c239b4f3feed7ea869` |
| `linux/run` | 0 | 0 | `65917e9f37317fec171f6a730cb306512d0e0434cada6bdad2325495c5ab9eef` |

Attempt 1 stopped in `cargo test --test reviewed_policy` because `OO_A227_OO_BUILDER` was unset. Bins, fetch, and the quantity source gate had exited 0. Diagnosis: `linux/run-attempt-1/DIAGNOSIS.txt`.

Attempt 2 reached `frozen_smoke=ok` and stopped in the harness compile. `classify.rs` passed a format hole with no argument. `catalog.rs` formatted a `char` with `:04x`, and `char` does not implement `LowerHex` on rustc 1.94.0. Diagnosis: `linux/run-attempt-2/DIAGNOSIS.txt`.

Attempt 3 reached smoke 0, harness build 0, lock-diff 0, and harness Clippy 0. The harness then stopped at `captured-pg-draft` because the catalog owner field was the leak marker, and the draft echoes that catalog fact. Diagnosis: `linux/run-attempt-3/DIAGNOSIS.txt`.

The successful run exports `OO_A227_OO_BUILDER` and `OO_A227_REVIEWED_EFFECTIVE` to the frozen binaries. The catalog owner is `lot_owner`. The marker `a259_owner_do_not_leak` is the preimage of a comment sha256. The draft must not contain that body.

## Frozen smoke, before the overlay

`linux/run/smoke.exit` is 0. Console contains `frozen_smoke=ok`.

| Test | Result |
|---|---|
| `reviewed_policy` | 21 passed, 1.29s |
| `reviewed_policy_cli` | 6 passed, 0.43s |
| `reviewed_policy_quantity` | 6 passed, 0.53s |

`cargo clippy --locked --offline -p oo-gateway --lib --bin reviewed-policy --test reviewed_policy_quantity -- -D warnings` exited 0.

## Independent harness

The harness is overlay member `a259-harness`, package `oo-a259-quantity-review`. `cargo build --offline` and harness Clippy omit `--locked` because the member is new. `linux/run/phases/lock-diff.exit` is 0. `linux/run/phases/clippy-harness.exit` is 0. `reviewed_policy.rs` in the overlay compared equal to the archive. Console contains `classify=ok` and `bound=ok`.

Library path, project `qtylot`, table `public.lot`, digest `fedcba9876543210` repeated, generation 13. That generation is neither 48 nor 1. Hand decimals parse as the `i16`, `i32`, and `i64` extremes and zero. `classify/policy.stdout` keeps confirmed quantity on `amount_i2`, `amount_i4`, and `amount_i8`, identifier on `batch_i8`, pii none on the first two amounts and on `batch_i8`, and declared `explicit_allow` true on personal `amount_i8`. `row_filter` is null. `compile_column_acl_set` accepts that one policy.

The same run refuses: missing `amount_i8` decision exit 10 `decision missing`; stale schema and stale fact exit 10; personal without explicit allow, and none with explicit allow, exit 12 `pii_allow`; quantity on `float4`, `float8`, `numeric`, `date`, `bool`, and `text` exit 11 `semantic`. Money on `int8` fails in `reviewed-effective-oos` with `oos.invalid` and `effective OOS validation failed`, empty stdout. Ratio, event time, date, audit time, geo, json, and system on `int8` exit 11. Those checks live in `harness/src/classify.rs`. A miss writes `stop.json` and a non-zero harness exit. The successful run has no `stop.json`.

## Live int8 chain

One table, `qty.ticket` (`ticket_id` int8, `amount` int8). Rows are `(1, i64::MIN)`, `(2, i64::MAX)`, `(3, 0)`. One role, `clerk`, login `oo_a259_login`. The requested import source cut is 48.

`live/generations.txt` reads:

```
source_generation=48
target_generation=1
run_generation=48
fence_generation=1
```

The harness reads `run_generation` from `_oo.physical_mappings` and the adopted generation from `_oo.target_generations`. It stops if the policy generation equals the fence or differs from the mapping run generation. Analyze uses source generation 48. The fence is not written onto the policy.

`live/reviewed-policy.stdout` has generation 48, digest `9675eb873e3222991d0759e1ac06460957ce144ea0ea2ad610cd139d03e2a8e6`, confirmed quantity and pii none on `amount`. Install and provision exited 0. Analyze exited 0 with `published` false and `estimate_is_exact` false. `query-ticket.exit` is 0. The three int cells are `1` / `-9223372036854775808`, `2` / `9223372036854775807`, and `3` / `0`. The summary is `succeeded`, `returned_rows` 3, `end_audit_ok` true.

`import-pg-not-ready.exit` is 1, which is the expected early refusal before `init-pg`.

Int2 and int4 quantity were proved through the producer and the CLI. They were not live queries.

## Still open

Money with a unit. Ratio, event time, date, audit time, geo, json, and system. Float, numeric, and date codecs. Quantity on text or bool. Policy generation copied from the target fence. Authentication, publication, and the audit WAL.

Reviews: `reviews/01-adapter-cell.md` through `reviews/04-scope.md`.
