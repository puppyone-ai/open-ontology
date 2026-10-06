# Review 1 — adapter cell

Scope: `crates/oo-gateway/src/reviewed_policy.rs` at `83b4f94a12c527ff8fd77eadd2909437e63d908c` against `contracts/gateway/reviewed-policy-quantity-v1.md` at `6c426aa5839fbebaf67b84d07cfd82b8e01391af`. No builder. The diff versus blob `56b62fdc1ab9864b99d992ccb46782fc34c1a507` is the `Quantity` alternative and two comments in `compatible`.

## Checks

`admit_one` returns `decision` `missing` when the allowlisted column has no decision. `check_decision` returns `stale_schema`, `unbound`, `stale_fact`, or `rejected` before `column_spec`. `check_output` returns `mismatch` or `seal` before `column_spec`. `column_spec` requires `data_type` and `physical_type` before `compatible`.

The integer arm is `int2`, `int4`, and `int8`, and it now includes `SemanticType::Quantity` beside identifier, foreign key, code, and status code. The `bool` arm is still `BooleanFlag` only. The `text` arm is the parent list and does not include quantity. The fallback is false, so `float4`, `float8`, `numeric`, and `date` stay refused. Money, ratio, event time, date, audit time, geo, json, and system are absent from every arm.

`semantic_name` already returned `"quantity"` on the fixed blob. The DTO therefore keeps the confirmed semantic name. No unit field was added. The function still does not read an import or a target fence. It copies `binding.generation`.

## Verdict

The committed cell matches the frozen contract. No further edit to `reviewed_policy.rs` from this review.
