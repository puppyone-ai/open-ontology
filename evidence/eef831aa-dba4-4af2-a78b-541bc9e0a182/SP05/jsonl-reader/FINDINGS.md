# Findings

Reviewed the owned reader, the contract, and the Linux logs for `47743688d45ec7a001e796725febc5357b7e20bd`.

## Accepted limits

- There is no decimal Arrow column. PostgreSQL decimal conversion stays private to the wire decoder. `ExactDecimal::parse_json_number` only accepts or refuses an `int64` lexical form. `1.0`, `1.5`, and `1e2` are `not_int64`. A pure integer outside `i64` is `int64_overflow`. There is no `f64` fallback.
- `-0` becomes integer `0`. `+1` is `type_mismatch`. A leading zero such as `01` is `invalid_number`.
- `json_text` stores the original value bytes, including nested objects and arrays and their whitespace. JSON `null` is an Arrow null, including on `json_text`. Nested values on `utf8`, `bool`, and `int64` are `nested_refused`.
- The profile has no PII or semantic-class field. Duplicate names, an empty selection, and ambiguous labels are refused.
- The whole-object digest is compared only after `Read` returns EOF at a record boundary. A delivered batch is not a completion token. `finish` before `JsonlItem::Exhausted` returns `not_exhausted`. A hash mismatch returns `hash_mismatch` and does not mint `JsonlComplete`.
- `JsonlItem::Batch` holds `Box<JsonlBatch>` so the exhaustion variant stays smaller than a record batch.
- The deadline is checked at open and before each read and record. A blocked `Read::read` is not interrupted.
- `admits_import`, `published`, `arrow_ipc`, and `executes_native` are false. This parser does not admit a database load.
- Cancel and drop zero the record window and drop the owned reader. There is no `into_inner`. Error display is `jsonl error: <kind> fact=<fact>` and carries no row bytes.
- The hash oracle in the test uses `sha2` at a separate call site and checks it against coreutils `sha256sum` for the file and for stdin. It is the same crate the reader uses, with an independent command.

## Preserved failures

- `linux/preedit` exit 101: the first archive listed `Cargo.toml`, `Cargo.lock`, `rust-toolchain.toml`, `.cargo`, and `crates`, and omitted `contracts/source/pgsql/empty-catalog.json`.
- `linux/focused` did not compile: `&String` versus `&[u8]`, `u64`/`u32` batch row counts, `CellVal` was not `Clone`, and a split borrow of the record window.
- `linux/focused-2` failed one test compile (`&u8` versus `u8` in the lineage helper).
- `linux/focused-3` failed two tests: `rows_emitted` counted a wide batch, and a budget with `max_total_bytes` below `max_record_bytes` was rejected as `max_total`.
- `linux/focused-4` tests and `--lib` passed. Clippy rejected `large_enum_variant` and `collapsible_if` in `jsonl.rs`.
- `linux/focused-5` tests passed. Clippy rejected `too_many_arguments`, `needless_lifetimes`, and `never_loop` in the test.
- `linux/focused-7` cleaned the package target and then failed because that new container had no clippy component.

## Dependency warning left in place

`sqlx-postgres` `message/flush.rs` and `message/mod.rs` emit `unfulfilled_lint_expectations` while the dependency is compiled. Those files are vendor code and were not edited. `cargo clippy -p oo-connectors --all-targets -- -D warnings` exited 0 in `linux/focused-8` after that warning text.

## Open work

Arrow IPC workers, multi-format input, DuckDB sniff, native isolation, runsc, and a file-hosting or object-store adapter are outside this reader. The `Read` API is what those adapters can wrap later.
