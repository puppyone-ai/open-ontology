# Findings

## Verdict

APPROVE_LIMITED for the metric-reconcile child at `15558917e2c5be12b74dd1d64fb542b7d05a03af`.

The limited approval is calculation readiness: a closed anchor, a reviewed enum class, typed binds, role `EXPLAIN`, a one-row test, and an exact decimal comparison against a separate control table.

Unapproved, and required to stay explicit: docs/06 §3.7 YAML `certified` (owner review), §6 publication (all of G1–G13), `owner_authenticated`, `full_gates`, the parent SP08 goal, and any release. The evaluated reports that set `metric_certified` true also set those three flags false and list the omitted gates as `not_run`. Reading `metric_certified` as a published or owner-certified metric would be wrong. The fresh proof does not do that, so the child is not withheld for a forged full certificate.

## Why the flag can be true while the certificate stays false

`CONTRACT.md` states that `owner_authenticated`, `publication`, `full_gates`, and `source_modified` are always false, and that `metric_certified` is true only for an evaluate report whose G5a and G5b are both `passed`, whose enum classes are reviewed at the closed anchor, whose control path is independent, and whose semantic diff does not require owner review. Compile never sets it.

The frozen `src/lib.rs` assigns it only as:

```text
g5a_status == "passed"
&& g5b.0 == "passed"
&& !semantic_diff.owner_review_required
&& enum_resolution.certification_allowed
```

docs/06 §3.7 says the YAML field `certified: true` exists only after owner review, and that a candidate metric stays `certified: false` until the owner certifies it. §3.7 also requires each certified metric to carry an owner-written `control_total` on a closed anchor. §6 says a publication tag is allowed only when every gate G1–G13 passes. G5a blocks a certified metric. G5b blocks publication, and a nightly miss degrades the metric.

The crate copies lifecycle `status`, `owner`, and `synonyms`. It does not copy YAML `certified`. The review request omitted any `certified:` key. `lifecycle.owner` is the text `ledger`. The same report has `owner_authenticated: false`. The owner string is not an authentication.

On `pass.json` and `scale.json` the component predicate holds, so `metric_certified` is true. On `tolerance.json` G5b is `failed` / `tolerance`, the difference is `0.01`, and `metric_certified` is false. G5a stays `passed`. Exit of an evaluated report is 0 in both cases, which matches the contract. That is limited calculation readiness. It is not docs/06 certification and it is not publication.

## Independent calculation

Fixture tables are `rev.ticket` (`gross`, `booked_on`, `kind`) and `ops.hand_close` (`closed_sum`, `anchor_day`). They are not the author's `src.receipt` / `ctl.period_total`.

Hand literal `1000000000000000000.01`. The last digit is present in the metric text returned by PostgreSQL. Scale text `1000000000000000000.010` is equal to `.01`; the reported difference is `0.000` and the metric text stays `.01`. The off-by-one-cent control `1000000000000000000.00` yields difference `0.01`, G5b `failed`, `metric_certified` false.

The review oracle recomputed those three differences with decimal digit subtraction at the wider scale. It does not use `f64`. The product comparison itself remains `oo_core::ExactDecimal`, as the author crate implements it. The two results agree on `0.00`, `0.01`, and `0.000`.

Receipts were recomputed in Rust from the `MetricReport` serde contract with `receipt_sha256` cleared, then SHA-256. They match the saved receipts.

## Binds, refusals, role

Executed SQL keeps `$1` `$2` `$3` on the metric and `$1` on the control. Caller dates and the hand literal are not spliced into that SQL. Probe sentinels `oo_mr_probe_` and `810316` are absent.

`unknown_class`, `same_path`, `anchor_open`, and `gap_anchor` refuse before any socket: exit 2, `database_used` false, `metric_certified` false, both executed SQL strings empty. `same_path` used the metric SQL as the control SQL. `gap_anchor` omitted the anchor date; the report's `anchor_date` is null.

Login `oo_a329_login` is `rolinherit=false` and is a member of `oo_r_review`. Direct `SELECT` returns `42501`. The component still read the one metric row under role `oo_r_review`, with explain `passed` and 2 rows. The author's live test, which used an inheriting login, was not run.

## Harness failures that are not product failures

`linux/review` archived the crate and the gateway tests and left out `examples/schema/metrics-finance.yaml`. `behavior.rs` line 204 `include_str!` then failed `cargo test` and clippy with exit 101. `sql_guard` had already passed, 24 tests. Live did not run. The product tree was not at fault.

`linux/review-2` included that YAML. Smoke, the 21 crate tests, and clippy exited 0. The review oracle then failed to compile: `host` and `report.outcome` were moved into `Bag::record` and used again. Live did not run. The oracle now passes those strings by reference. `linux/review-3` repeated smoke, tests, and clippy before the live oracle, and that oracle exited 0.

## Limits that remain

G1–G4 and G6–G13 did not run. There is no nightly, no customer control-total job, and no owner signing the YAML `certified` flag. `full_gates` is false. A later publisher must not treat these JSON files as a G1–G13 pass.
