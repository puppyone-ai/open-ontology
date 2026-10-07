# Numeric query cell

Contract `oo.gateway.numeric-query/v1`. Task a327. Session `eef831aa-dba4-4af2-a78b-541bc9e0a327`. Fixed ancestor `e6ebf93b5ac261c097a24d145611e0300e4f88b5`.

This file freezes the numeric wire, the reviewed-policy admission cell, and the query cell JSON. It is a child of `contracts/gateway/reviewed-policy-quantity-v1.md`, `contracts/gateway/query-executor-v1.md`, and `contracts/gateway/query-pg-cli-v1.md`. Those parent files are not edited. The quantity parent still says `numeric` stays `semantic`. This child amends only `numeric` plus semantic `quantity`.

This contract does not close M1-D14B, M1-D14C, M1-I01, or M1-I02, and it does not release.

`SemanticType` has `quantity` and `money`. It has no `measure` variant and no `amount` variant. The documented semantic for the quantity, measure, and amount cell in the current enum is `quantity`. That is the semantic a317 confirmed for `shop.entry.amount`.

## Admission

After the existing decision, seal, classification, PII, and row-security checks have passed, `base_type` also accepts the physical name `numeric`, and `compatible` accepts that name only with `SemanticType::Quantity`.

The physical row must be `typtype` `b`, and `type_name` must equal the effective column `data_type`, and both strings must be exactly `numeric`. `numeric(18,4)` is not that name. A domain (`typtype` other than `b`), an array, or any other type name stays `semantic`.

`int2`, `int4`, and `int8` keep the quantity cell they already have. `bool` and `text` do not gain `quantity`. `money`, `ratio`, `identifier`, `foreign_key`, `code`, `status_code`, and every other semantic stay refused on `numeric`. Quantity still has no unit constraint. This cell does not reclassify quantity as an identifier.

`float4`, `float8`, `date`, `varchar`, `unknown`, and every other non-admitted physical type stay refused. PII mapping, the config seal, the captured catalog, classification, and the column ACL compiler are unchanged.

## Wire

OID `1700` only. The payload is big-endian: `ndigits` `int16`, `weight` `int16`, `sign` `uint16`, `dscale` `int16`, then `ndigits` base-10000 digits. Each digit is an `int16` in `0..=9999`.

Signs: positive `0x0000`, negative `0x4000`, NaN `0xC000`, positive infinity `0xD000`, negative infinity `0xF000`.

`ndigits` is read as a signed `int16`. A negative `ndigits` is `numeric_payload`. It is not cast to `u16`. A negative `dscale`, a header shorter than 8 bytes, a length other than `8 + ndigits * 2`, a digit outside `0..=9999`, or any other sign is `numeric_payload`.

A structurally valid special is explicit and fail-closed:

| Sign | Protocol step |
|---|---|
| `0xC000` | `numeric_nan` |
| `0xD000` | `numeric_infinity` |
| `0xF000` | `numeric_neg_infinity` |

`weight` is applied into the coefficient and the scale before `ExactDecimal::new`. The cell has no exponent field. `point = (weight + 1) * 4` uses checked arithmetic. If that integer-part width, the display scale, or the coefficient length would exceed `CANONICAL_MAX_COEFFICIENT_DIGITS` (`8192`), the step is `numeric_precision` and the decoder does not allocate past that budget. `ExactDecimal::new` returning `Budget` is the same step.

Empty digits are unsigned zero at scale `dscale`. `ExactDecimal::new` clears the sign on a zero coefficient, so negative zero stays unsigned. Display scale pads trailing zeros: `1` at `dscale` 4 is coefficient `10000`, scale `4`, plain text `1.0000`. A non-zero fractional digit past `dscale` is `numeric_scale`. Extra fractional zeros are truncated to `dscale`.

No `f64`, no floating cast, and no text round-trip through PostgreSQL `numeric` output. The decoder calls `oo_core::ExactDecimal` only.

## Typmod

The column `type_modifier` is decoded with the PostgreSQL 11-bit signed scale rule, the same rule as `numeric_typmod_v1`: `typmod < 0` means unconstrained; otherwise `adjusted = typmod.wrapping_sub(4)`, `precision = (adjusted >> 16) & 0xFFFF`, and `scale = ((adjusted & 0x7FF) ^ 0x400) - 0x400`. The unsigned low-16 catalog helper is not used.

Unconstrained `numeric` (`typmod < 0`) is limited only by the 8192-digit budget. A non-negative typmod does not rescale. The decoded scale must equal the declared scale, or the step is `numeric_scale`. The coefficient length must be `<= precision`, or the step is `numeric_precision`. The check uses `coefficient().len()`, not `storage_precision()`. `storage_precision()` would reject a true `numeric(4,6)` value whose coefficient is `1234` at scale `6`.

`numeric(18,4)` is typmod `1179656`. A negative declared scale that the binary `dscale` does not equal fails `numeric_scale`.

## Query cell

`Cell::Decimal(ExactDecimal)` is added beside `Null`, `Bool`, `Int`, and `Text`. SQL NULL of a numeric column stays `Cell::Null`.

`query-pg` stdout adds one internally tagged object. Existing kinds stay as they are.

```json
{"kind":"decimal","negative":false,"coefficient":"123456789012345678","scale":4}
```

`negative` is a bool. `coefficient` is a digit string with no sign and no decimal point. `scale` is the `i32` already stored on `ExactDecimal`. `result_bytes` for this cell is the length of `ExactDecimal`'s `Display`: a minus sign when negative, the point, and the trailing zeros. Null is 0 bytes, `true` is 4, and `false` is 5, as before.

No `StopClass` variant is added. A refused OID stays `TypeRefused` and the summary stop stays `type_refused`. A numeric decode failure is `ExecError::Protocol` with one of the static steps above, and the summary stop stays `protocol`. The step is not written to stdout or stderr. `query-pg` still exits 8 with stderr `query-pg: query`. Summary keys are unchanged.

OID set after this contract: bool `16`, int8 `20`, int2 `21`, int4 `23`, text `25`, numeric `1700`. `float4` `700`, `float8` `701`, `varchar` `1043`, `date` `1082`, and `unknown` `705` stay `TypeRefused`.

The owned bootstrap test that asserted `oid_allowed` false for OID 1700 now asserts true. A still-refused OID, `varchar` `1043` or `float4` `700`, demonstrates `TypeRefused`. The existing error-code assertion may still construct `TypeRefused { oid: 1700 }`; that assertion is about `ErrorCode`, not admission.

## Hand literals

These values are independent of the decoder output. `12345678901234.5678` is not an exact `f64`.

| Value | `negative` | `coefficient` | `scale` | plain text |
|---|---|---|---|---|
| `12345678901234.5678` | false | `123456789012345678` | 4 | `12345678901234.5678` |
| `-12345678901234.5678` | true | `123456789012345678` | 4 | `-12345678901234.5678` |
| `0.0001` | false | `1` | 4 | `0.0001` |
| `-0.0001` | true | `1` | 4 | `-0.0001` |
| `0.0000` | false | `0` | 4 | `0.0000` |
| NULL | | | | kind `null` |

Reviewed wire vectors, plain text after `Display`:

| `weight` | sign | `dscale` | digits | plain text |
|---|---|---|---|---|
| 4 | positive | 2 | `100,0,0,0,0,100` | `1000000000000000000.01` |
| 0 | positive | 2 | `1,100` | `1.01` |
| -1 | positive | 2 | `100` | `0.01` |
| 0 | positive | 2 | empty | `0.00` |
| 0 | negative | 2 | `1,100` | `-1.01` |
| -2 | positive | 6 | `100` | `0.000001` |
| 0 | positive | 4 | `1` | `1.0000` |

`weight` 0, positive, `dscale` 1, digits `1,100` is `numeric_scale`. Digit `10000`, a short payload, NaN, and either infinity are refused.

## Seams that this task does not edit

- `crates/oo-gateway/tests/query_executor_pg_live.rs` `canon_len` matches `Cell` exhaustively and will not compile once `Cell::Decimal` exists.
- `crates/oo-gateway/tests/reviewed_policy_quantity.rs` builds its `numeric` column with `typmod` `-1` and classifier reason `unbounded_or_wide_numeric`. The draft leaves `data_type` null for that column, so `column_spec` still returns `semantic` before this cell. That test still passes. It does not cover `numeric(18,4)`. Owned tests cover bounded `numeric` plus `quantity`. `float4`, `float8`, `date`, `bool`, and `text` stay refused there and in the owned tests.
- `benchmarks/query-result-adapter/src/parse.rs` rejects an unknown `kind`, including `decimal`.
- `crates/oo-gateway/tests/query_pg_cli.rs` `row_canonical_bytes` panics on an unknown JSON kind. Existing fixtures are not decimal, so that file still compiles.
- `testing/real-db/pg-e2e` records the probe label `numeric_cell_set=null_bool_int_text`. This task does not change that driver.

## Proof this task runs

Owned unit tests decode the wire vectors, the typmod checks, the specials, the budget refusal, and the admission refusals. `query-pg` unit tests lock the existing JSON kinds and the decimal object.

The private TLS proof uses PostgreSQL 17.10, compiles `reviewed-policy`, the role pack, and `query-pg` from the fixed archive plus this overlay, and compares query stdout with the hand literals above. NaN, positive infinity, and negative infinity are inserted into an admitted `numeric(18,4)` column and selected: exit 8, summary stop `protocol`, no decimal cell. A role without the column is a guard or database denial, not a decimal cell. The proof does not print SQL.
