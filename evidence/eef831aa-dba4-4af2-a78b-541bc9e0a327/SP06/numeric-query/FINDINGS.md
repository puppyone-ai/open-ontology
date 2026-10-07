# Findings

## Admission of numeric quantity is live

`reviewed-effective-oos` applied `data_type` `numeric` and semantic `quantity` to `shop.entry.amount` and `shop.entry.wide`. `reviewed-policy` then emitted one policy. Money, identifier, and the other non-quantity semantics stay refused in the unit tests. Integer quantity is unchanged. `float4`, `float8`, `date`, `varchar`, and `unknown` stay refused.

An unbounded numeric column still has no `data_type`. `column_spec` returns semantic before `compatible`. The unowned test `wrong_physical_quantity_is_semantic` therefore still passes. That is the behavior recorded in `execution/decisions/numeric-query-contract-a327.md`.

## Exact values import

`import-pg` exit 0 for six `shop.entry` rows. The values are the independent literals: `12345678901234.5678`, its negative, `0.0001`, `-0.0001`, `0.0000`, and null, plus the same pattern for `numeric(38,4)` with coefficient `12345678901234567890123456789012345678`. PostgreSQL accepted those inserts into the declared types. The import copied them. They were not inserted into the target by hand.

## Infinity and NaN do not reach a numeric(18,4) query

`numeric(18,4)` rejects `'Infinity'::numeric` at insert. PostgreSQL reports numeric field overflow, SQLSTATE 22003: precision 18 scale 4 cannot hold an infinite value. `'-Infinity'` is the same class. NaN does insert into `numeric(18,4)`, and the approved import then refuses it with `full import refused numeric_nan`. The target was not patched by hand.

The decoder already fail-closes those wires in unit tests: `numeric_nan`, `numeric_infinity`, and `numeric_neg_infinity`. A live `query-pg` row of those values was not available through the approved import.

## Query cells are blocked in unowned catalog code

`install-column-acl` exit 7. stderr is exactly:

```
install-column-acl: policy type
```

`ensure_grantable_type` in `crates/oo-gateway/src/policy.rs` accepts a base type only when `policy_catalog::SUPPORTED_TYPES` contains the name. On the fixed archive that list is `bool`, `int2`, `int4`, `int8`, `text`.

`load_relation` in `crates/oo-gateway/src/policy_catalog.rs` returns `CatalogError::UnsupportedType` for a published column that fails the same check. After a grant, `query-pg` would still stop in catalog load with policy exit 5, before `decode_cell` and before a decimal JSON line.

Both files are outside the a327 owned set. This task did not edit them. Adding `numeric` there is a separate change. It has to be frozen and owned before it is coded. The a327 harness still expects six exact decimal rows, so it fails at this boundary on purpose.

## Earlier compile seam

The post-edit run compiled `query_executor_pg_live` with exit 101. The log recorded a missing `Cell::Decimal` arm. That file is now dirty in the working tree and was not written by this task. The proof container used the fixed archive, which does not include a later edit to that test.

## Builder notes

`linux/live-failed-1` is a false green: `--exact live_numeric_proof` matched no test, and cargo exited 0. The rerun uses `--exact a327_live::live_numeric_proof` and requires `outcome.txt`. The later runs fail before that file is written.

`role-pack-bundle` publishes with a hard link. The bundle file was written on the target volume, where the link succeeds.

No password bytes were found under this evidence tree. Request files contain statement text because `query-pg` reads them. They are not quoted here. Query stdout, stderr, and audit were not created.
