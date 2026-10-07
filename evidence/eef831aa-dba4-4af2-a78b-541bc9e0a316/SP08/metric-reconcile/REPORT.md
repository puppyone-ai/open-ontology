# SEM-D05 / E42 metric-reconcile

UUID `eef831aa-dba4-4af2-a78b-541bc9e0a316`. Component only: compile one core-valid metric and compare it with a separate operator control statement on a private TLS PostgreSQL. This is not full SP08 certification, not a nightly, and not a customer control-total program.

Fixed archive `99ac19af42059a308ba9c418815619e5a081f407`. Root `Cargo.lock` stayed `16e6afda29ae4bae251d569f3c4ad16459527f0d0323598d8e4e0971818d458a` before and after every builder. The child lock is `tools/metric-reconcile/Cargo.lock` (`59599e433b3c025a0ec739989c0f9df74eebcc2f6cb9f9a96892523a10048948`). Registry pins in that lock are a subset of the fixed root lock. `cargo update` was not run.

## Proof

Linux arm64 Rust `1.94.0` image `docker.io/library/rust@sha256:0f24958fa693fe838859faf64a0ec78d382cd543a6b6efd0fa7a3bcb10ea90d5`. PostgreSQL 17.10 image `docker.io/library/postgres@sha256:3aac91d19d5b7aff4c650dddfacc1a26c436459aafd4eb51ab0bf1748e990dbb`. Builder 6 GiB, 1 CPU, `cap-drop ALL`. Database container 1 GiB, 1 CPU, no host port, scram, TLS SAN `DNS:oo-a316-pg`.

Pre-edit smoke of fixed-archive `oo-gateway` `sql_guard`: 24 passed, exit 0 (`linux/preoverlay`).

Final builder `linux/overlay-4` (container removed, exit 0, not OOM, not privileged, memory 6442450944, NanoCpus 1000000000):

- `cargo test --locked --offline`: 21 passed, 0 failed. The live test early-returns in this phase.
- `cargo clippy --locked --offline --all-targets -- -D warnings`: exit 0.
- Live test `hand_literal_independent_control` with `OO_A316_LIVE=1`: 1 passed, 20 filtered out.

The database still held the rows after that test: `src.receipt.amount::text` = `1000000000000000000.01` and `ctl.period_total.total::text` = `1000000000000000000.010`. Roles `oo_r_finance` and `oo_a316_login` existed. The two amounts were inserted by separate statements.

Saved reports are under `linux/overlay-4/live-reports/`. Receipts were recomputed from the saved JSON with `receipt_sha256` cleared and matched.

| run | G5a | G5b | metric value | control value | difference | metric_certified | receipt_sha256 |
|---|---|---|---|---|---|---|---|
| pass.json | passed ok | passed ok | 1000000000000000000.01 | 1000000000000000000.01 | 0.00 | true | `0fb55400c620823a6cea9272beb695f43e4ff8aa6386c229ed4f39b90ddf91c0` |
| tolerance.json | passed ok | failed tolerance | 1000000000000000000.01 | 1000000000000000000.00 | 0.01 | false | `92d7a61d1da483318036391aa8c1a3a399c58ab5d75df5d83cd305da62bd965b` |
| scale.json | passed ok | passed ok | 1000000000000000000.01 | 1000000000000000000.010 | 0.000 | true | `c2d289dd2d171c7e3127c246453b3c65bc080c5386e38ffd17ebecce7a3f9dae` |

On all three: `outcome` evaluated, `owner_authenticated` false, `publication` false, `full_gates` false, `database_used` true, `source_modified` false, `password_logged` false, explain 2 rows, test actual_rows 1, omitted G1–G4 and G6–G13 `not_run`. Metric fingerprint `573e55c0b76f18ed`. Control fingerprint `db0e6b1a65fda118`. Executed SQL keeps `$1`/`$2`/`$3` and does not contain the caller dates or the hand literal.

`metric_certified` true here means this component's predicate only: parse, the declared test, role `EXPLAIN`, G5b, reviewed enum class `confirmed` code `C`, independent control, and no owner-review diff. It does not mean the metric is published or that every SP08 gate ran.

## Attempts kept

| dir | result |
|---|---|
| linux/preoverlay | smoke exit 0 |
| linux/overlay | compile exit 101, five type errors, lock unchanged |
| linux/overlay-2 | tests 19 passed / 2 failed, clippy exit 101 |
| linux/overlay-3 | tests, clippy, and live exit 0, reports not saved |
| linux/overlay-4 | same gates, three reports saved |
| pg | `up.exit` 0 |

Launch shape follows this task's preoverlay script, which records a204 overlay-3 `78c8ef8c179c59317850c1b6a9ae53d3f883b8a1b24c2567fa0d3d26f1997eee`. PostgreSQL scripts follow a200 `up.sh` `96613a159f6c05b4a8d8d1fb79bb2613ae9b3e9a00a7b942196548a00964da45` and `pg-wrap.sh` `25712207f986b11e122bd992d5a695881e1e86625e0edf2d0b683b023ebfdd2f`.
