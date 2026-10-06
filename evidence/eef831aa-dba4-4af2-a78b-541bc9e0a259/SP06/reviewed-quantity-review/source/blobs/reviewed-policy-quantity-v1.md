# Reviewed integer quantity

Contract `oo.gateway.reviewed-policy-quantity/v1`. Child of `contracts/gateway/reviewed-policy-v1.md`. Library `oo_gateway::reviewed_policy`.

This file freezes one compatibility cell. It does not replace the parent contract, it does not add a component, and it does not close M1-D14B, M1-D14C, M1-I01, or M1-I02. The parent sentence that lists `quantity` as `semantic` is the historical limited acceptance of that adapter. This child amends only the cell below. The parent file is not edited.

## What is accepted

After the parent checks 1 through 15 have passed for the allowlisted column, `int2`, `int4`, and `int8` also accept semantic type `quantity`.

Those checks include the sealed captured-draft artifact, the decision hash, and the current decision. A quantity label is not a bypass. `missing`, `stale_schema`, `stale_fact`, `rejected`, `unbound`, `unknown`, and `mismatch` still refuse as `decision` before this cell is considered. A hidden column after a successful bind is still `seal`.

The physical type is the sealed catalog row: `typtype` `b`, `type_name` equal to the effective column `data_type`, and one of `int2`, `int4`, `int8`. The column keeps the name and the semantic type the decision confirmed. This contract does not reclassify `quantity` as `identifier`, `code`, `foreign_key`, or `status_code`. An amount column that the decision confirmed is not dropped from the fixture or the allowlist to make the set compile.

`identifier`, `foreign_key`, `code`, and `status_code` on those integer types stay accepted. `bool` still accepts only `boolean_flag`. `text` keeps the parent text list. Neither `bool` nor `text` accepts `quantity`.

PII is unchanged. `none` requires `explicit_allow` false and becomes `PiiClass::None`. `personal`, `sensitive`, and `restricted` require `explicit_allow` true and become `PiiClass::Declared { explicit_allow: true }`. The other combinations are `pii_allow`. Declared PII is not downgraded to `none`.

`compile_column_acl_set` still runs on the resulting set. `row_filter` stays `None`. The adapter still does not open a database, install, grant, or publish.

## What stays refused

`float4`, `float8`, `numeric`, `date`, and any other type outside `bool`, `int2`, `int4`, `int8`, and `text` stay `semantic`. That refusal is the existing base-type check. This child does not add a physical codec for them.

`money`, `ratio`, `event_time`, `date`, `audit_time`, `geo`, `json`, and `system` stay `semantic` on every physical type this adapter accepts. Core `validate_dataset` requires `unit` only when `semantic_type` is `money`. `quantity` has no unit constraint there, so this child does not require a unit, a currency, a scale, or a ratio. Those semantics are not added.

A missing `data_type`, a non-base `typtype`, or a catalog `type_name` that differs from `data_type` stays `semantic`. Row security on an allowlisted table stays `rls`.

## Generation

This adapter copies `binding.generation` onto every policy. It does not read an import, a mapping table, or a target fence, and it does not invent a generation.

A composition that installs the DTO with the existing column-ACL installer must set that binding from `max(run_generation)` in `_oo.physical_mappings` for the policy project and config digest. That stored run generation is the source cut the importer registered. The import result's `target_generation`, confirmed on `_oo.full_import_run.target_generation`, is a different integer. On the reviewed source cut those values are source generation 48 and target generation 1. They are not interchangeable.

Stamping the source generation into the target-generation field, or stamping the target fence generation onto the policy because it was nearby in the import result, is outside this adapter. The composition proof must read both numbers from the import result and the mapping rows and must refuse a run that writes one into the other's field.

## Proof

`crates/oo-gateway/tests/reviewed_policy_quantity.rs` calls the compiled `captured-pg-draft`, `reviewed-effective-oos`, and `reviewed-policy` binaries.

The positive fixture names `amount_i2`, `amount_i4`, and `amount_i8` as `quantity` on `int2`, `int4`, and `int8`, plus an `int8` `identifier` so the older integer cell remains. Hand amounts, not a generated series, are the integer extremes and zero:

| Column | Values |
|---|---|
| `amount_i2` | `-32768`, `32767`, `0` |
| `amount_i4` | `-2147483648`, `2147483647`, `0` |
| `amount_i8` | `-9223372036854775808`, `9223372036854775807`, `0` |

The library test checks those decimals. It does not query a database. The private TLS composition, when run, installs the reviewed-policy DTO through the existing installer, provisions the mapping reader, analyzes, and queries. The integer text above must come back unchanged. The policy generation in that run is the mapping `run_generation`. The target fence generation is recorded separately.

Negative cases are `quantity` on `float4`, `float8`, `numeric`, `date`, `bool`, and `text`; a missing decision; a stale schema hash; a stale fact fingerprint; `money`, `ratio`, and `event_time` on `int8`; and a PII token that disagrees with `explicit_allow`.

## What stays open

Authentication, grants, publication, an audit WAL, and live row-level security stay open. This contract does not accept a parent plan item and it does not release.

Next seams, not implemented here:

- `money` with the core `unit` constraint, on a physical type this adapter can already read
- `ratio`, `event_time`, `date`, `audit_time`, `geo`, `json`, and `system`
- `float4`, `float8`, `numeric`, and `date` codecs
- `quantity` on `text` or `bool`
- a policy generation taken from the target fence rather than mapping `run_generation`, which the existing installer still rejects
