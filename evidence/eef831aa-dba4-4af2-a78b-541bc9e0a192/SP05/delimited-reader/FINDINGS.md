# Findings

Reviewed the owned reader, the contract, and the Linux logs for `671507b229ad7d1bc6a2e948b5b74d6d7252393f`.

## Accepted limits

- Dialect is explicit. `DelimitedDialect::try_from_separator` accepts comma and tab. Every other byte is `config` / `ambiguous_dialect`. The quote is `"`. Escape is a doubled quote. A backslash is ordinary text.
- The record window is allocated once at `open` with length `max_record_bytes` (floor 3, so the BOM lookahead fits). `open` does not read. The window is not grown. Field content is capped before each extra byte is stored.
- Strict UTF-8. BOM policy is `refuse`, `strip`, or `require` for `EF BB BF` at offset 0 only. Those three bytes are hashed. After strip or require, the first record offset is 3. The BOM does not add a line.
- `header=exact` compares decoded names with the profile. Header fields are not typed and are not subject to the null policy. Width is checked before name equality. A missing exact header, including an empty object, is `header_required` and is checked before the hash. An absent-header empty object can complete when the hash matches. Zero data rows after a matched header can complete.
- `NullPolicy::Empty` treats an unquoted empty field as null and a quoted empty field as an empty string. `NullPolicy::Token` treats only the unquoted token as null. A quoted token stays literal text. An empty string on `bool`, `int64`, or `decimal` is `empty_field`. On `utf8` it is presence `Empty`, distinct from null.
- `int64` and `decimal` reject `+`, whitespace, exponents, and leading zeros before calling `oo_core::ExactDecimal`. `parse_sql` would trim and accept `+`, so the lexical gate is in front of that call. `-0` and `-0.00` collapse to a non-negative zero. `1.2` and `1.20` at scale 2 are coefficient `120`. `1.251` at scale 2 is `inexact_decimal`. `1200` at scale `-2` is coefficient `12`. `12` at scale `-2` is `inexact_decimal`. A coefficient past precision after rescale is `decimal_precision`. `fits_decimal128` failure is `decimal_storage`. 8193 nines reach `number_budget`.
- There is no `f64`. Arrow decimal uses the public `Decimal128Array` constructor. Private PostgreSQL binary decoder constructors are not used.
- The whole-object digest is compared only after `Read` returns EOF at a record boundary. A delivered batch is not a completion token. `finish` before `DelimitedItem::Exhausted` returns `not_exhausted`. A hash mismatch returns the pending batch once, then `hash_mismatch`, and does not mint `DelimitedComplete`. Past `max_total_bytes`, one extra byte is probed and left unhashed.
- Cancel and the deadline run in the guard before each `read`. A blocked `Read::read` is not interrupted. The hold fixture proves the in-flight `read` still returns (`reads == 1`) and that `finish` then returns `cancelled`. A deadline that is already due is accepted by `try_new` and rejected by `open` before any read.
- `admits_import`, `published`, `arrow_ipc`, and `executes_native` are false. This parser does not admit a database load.
- Cancel and drop zero the record window and drop the owned reader. Error display is `delimited error: <kind> fact=<fact>` and carries no row bytes. I/O errors drop the OS string (`SECRET_OS_a192` stays out of the fact).
- The product hash oracle is the Rust FIPS implementation in the test, checked against the empty and `abc` vectors, plus `sha2` inside the reader. `sha256sum` appears only in the launch scripts as the readonly archive identity check.
- `columns_uncovered` remains a finish backstop. The empty exact-header path fails earlier as `header_required`.

## Preserved failures

- `linux/preedit` exit 101: the first archive listed `Cargo.toml`, `Cargo.lock`, `rust-toolchain.toml`, `.cargo`, and `crates`, and omitted `contracts/source/pgsql/empty-catalog.json`. Container `249e9f0ce2b5`. This is an archive input gap.
- `linux/focused` exit 101: four borrow errors in `delimited.rs` (`build_batch`, the window read in `pull_bytes`, and `type_record` / `classify`). Container `29250354ee0a`.
- `linux/focused-2`: `--lib` 64 passed. The delimited test did not compile. The SHA-256 state array was inferred as `{integer}`, and a hand-checked row tuple was read at field 4. Container `434d1df70433`.
- `linux/focused-3`: `--lib` 64 passed and `--test delimited` 25 passed. Clippy `-D warnings` rejected `vec_box`, `needless_range_loop`, and `io_other_error` in the test. Container `addbb024faa8`.

Those directories, including `console.log` and the exit files, were left as recorded.

## Dependency warning left in place

`sqlx-postgres` `message/flush.rs` and `message/mod.rs` emit `unfulfilled_lint_expectations` while the dependency is compiled. Those files are vendor code and were not edited. `cargo clippy -p oo-connectors --all-targets -- -D warnings` exited 0 in `linux/focused-4` after that warning text. Clippy stderr ends with `Finished dev profile [unoptimized + debuginfo] target(s) in 1.24s`. Clippy stdout is empty.

## Open work

GB18030, other charsets, delimiter sniffing, DuckDB, Arrow IPC, worker isolation, file hosting, the loader, and publication are outside this reader. The `Read` API is what those adapters can wrap later. A separate fixed review of `671507b229ad7d1bc6a2e948b5b74d6d7252393f` has not run.
