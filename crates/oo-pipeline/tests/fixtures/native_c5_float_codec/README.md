# Finite-float C5 canonical contract

Helpers and pipeline export are implemented; the coordinator wires the producer
and retained-baseline digest paths. Do not substitute a full-capture fallback
after an error.

```rust
native_c5_float::canonical(value: f64) -> Result<String, &'static str>
c5_float_sql::canonical_expression(column: &str) -> Result<String, &'static str>
```

The pipeline module is exported as `oo_pipeline::native_c5_float`.

## Integration handoff

- Kant (`01a173a9-6cc7-7bc0-bc3d-34c94b5cb27b`): FLOAT4/FLOAT8 SQL hashing
  uses `c5_float_sql::canonical_expression(&column.plan.name)`. Producer-side
  Arrow hashing uses the shared pipeline helper, not a second formatter.
- Anscombe (`01a173a9-369f-7f31-a09f-7dce9dfe9d67`): retained
  `NativeCell::Float64(bits)` hashing uses
  `oo_pipeline::native_c5_float::canonical(f64::from_bits(*bits))?.into_bytes()`.
- Both retain the existing separate NULL flag. Do not narrow an f64 to f32;
  the FLOAT4 Arrow value is already widened exactly. Propagate errors rather
  than hashing an empty/NULL value or falling back to a full capture.
- Module export and helper qualification are complete; full producer/state
  integration and approved-validator acceptance belong to their callers.

The output is `[-]digits e exponent`: the digits are an integer coefficient.
Examples: `1.23 -> 123e-2`, `1e20 -> 1e20`, `0 -> 0e0`, `-0 -> -0e0`.
Nonzero coefficients have neither leading nor trailing zeroes. Only textual
zero removal and decimal-point/exponent relocation occur; no value rounding.
FLOAT4 must be widened exactly from f32 to f64. DOUBLE uses its original bits.

The local helper normalizes `serde_json`'s f64 representation and checks its
roundtrip against the original IEEE bits. It rejects NaN and infinities.
Its internal lexer rejects malformed JSON numbers, oversized strings,
nonzero underflow, overflow and coefficients exceeding 17 significant digits.
If two successful canonical outputs are equal, parsing that common string
recovers both original bit patterns; therefore those patterns are equal.

The SQL helper accepts only 1..64 ASCII letters/digits/underscores and quotes
the identifier. It extracts the FLOAT/DOUBLE JSON atom via
`JSON_UNQUOTE(JSON_EXTRACT(JSON_ARRAY(column),'$[0]'))`, normalizes its text
without FLOAT-to-DECIMAL conversion, and fixes ASCII collation explicitly.
SQL NULL remains NULL for the caller's existing null flag. Other numeric
domains, strings and invalid text raise a JSON error rather than becoming a
NULL/empty digest. No `FORMAT`, rounding, or MySQL 8 `REGEXP_REPLACE` is used.

## Repeatable tests

The isolated fixture Cargo package uses the same serde_json/formatter versions
as the main product, without building or editing the main module wiring.

```sh
cargo test --offline --locked \
  --manifest-path product/crates/oo-pipeline/tests/fixtures/native_c5_float_codec/Cargo.toml \
  -- --nocapture

OO_C5_FLOAT_OWNED=1 cargo test --offline --locked \
  --manifest-path product/crates/oo-pipeline/tests/fixtures/native_c5_float_codec/Cargo.toml \
  owned_mysql57_source_local_parity -- --ignored --nocapture
```

The live test has a fixed container name
`oo-c5-incremental-20261025-fixture-mysql`, uses only database
`float_codec_fixture`, and leaves uniquely named synthetic FLOAT/DOUBLE tables
as evidence. The fixture database must already exist. No other database is
read or changed. SQL inserts force floating literals rather than DECIMAL
literals, and every retrieved source JSON and canonical string must parse
back to the generated bit pattern (exact f32 widening for FLOAT).

Coverage includes ULP neighbors, subnormal and normal boundaries, both zero
signs, powers of two/ten, deterministic random finite bits, collision checks,
invalid domains, and latin1/utf8/utf8mb4 connection collations.

The real server tested is the owned MySQL 5.7.44 fixture. This does not claim a
live MySQL 5.7.28 customer qualification, an approved-validator run, or the
coordinator's producer/baseline/Gateway integration acceptance.
