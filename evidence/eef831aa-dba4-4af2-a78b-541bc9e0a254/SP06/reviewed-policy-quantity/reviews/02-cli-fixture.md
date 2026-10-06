# Review 2 — CLI fixture

Scope: `crates/oo-gateway/tests/reviewed_policy_quantity.rs` at `e447249ab1a7f4c82df4729090b1285c9ef4a4ec`, blob `b31773e9cb61d73c88b217225aa1d5b6de140fa9`, against the contract proof section. The existing `reviewed_policy.rs` and `reviewed_policy_cli.rs` tests were not edited. Linux post-edit log: 6 passed, 0 failed, 0.55s, after the `into_*` rename. Clippy `-D warnings` on this test exited 0 in `linux/postedit/phases/clippy.exit`.

## Checks

Hand constants are the int16, int32, and int64 extremes and zero, and `hand_amount_bounds_are_the_integer_extremes` parses them. The columns are named `amount_i2`, `amount_i4`, and `amount_i8` with semantic type quantity. `batch_i8` stays identifier. The producer assertion requires `semantic_type` `quantity` before the CLI runs. The DTO assertion requires those names to remain, confirmed status, the PII classes in the contract, `row_filter` none, and generation 7, which the test also asserts is neither 48 nor 1. `compile_column_acl_set` is called on the parsed policies.

PII mismatches exit 12 with `reviewed-policy: pii_allow`. Quantity on float4, float8, numeric, date, bool, and text exits 11 with `reviewed-policy: semantic`. Numeric rows set `type_support.reason` to `unbounded_or_wide_numeric`. Missing, stale schema (`ef` times 32), and stale fact (`ab` times 32) exit 10 with the matching decision reason. Money fails in `reviewed-effective-oos` with `oos.invalid` and `effective OOS validation failed`. Ratio, event time, date, audit time, geo, json, and system exit 11.

The file has no database client. Env names are `OO_A254_OO_BUILDER`, `OO_A254_REVIEWED_EFFECTIVE`, and `OO_A254_REVIEWED_POLICY`.

## Verdict

The fixture covers the contract's library proof and the listed refusals. No further edit from this review. The live int2 and int4 query gap is review 3, because this file is specified not to query a database.
