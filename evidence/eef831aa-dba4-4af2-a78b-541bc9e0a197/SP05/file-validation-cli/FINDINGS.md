# Findings

Author review of `validate-file` at `59be14baab1d5f30abaa2ce8cf67d607a2e4417f`, the contract, and the Linux logs. This review checks the command’s integration with the public reader constructors. It does not accept delimited reader `671507b229ad7d1bc6a2e948b5b74d6d7252393f`. That commit stays author-only until its own fixed review.

## Integration that the focused-3 process checked

- The binary is the compiled `validate-file` from `CARGO_BIN_EXE_validate_file` (hyphen form accepted if that is the name Cargo sets). The old `oo-pipeline` binary is spawned the same way for the import regression.
- CSV, TSV, and JSONL requests are built as JSON and passed on the command line as `--request`. The test also builds the same profile with `DelimitedProfile::try_new` / `JsonlProfile::try_new` and pulls the same file through `DelimitedReader` / `JsonlReader`.
- The hand-written profile preimage, the library `profile_preimage` / `profile_digest` / `config_identity`, and the CLI hex fields agree. JSONL exposes digests only. The test hashes the documented preimage itself and compares `profile_digest` and `config_identity`.
- CSV covers utf8, bool, int64, and decimal(10,2), a BOM that is stripped, an exact header, a quoted comma, a quoted empty field, an unquoted empty null, `i64::MIN`, `i64::MAX`, and the texts `1.2`, `-0.00`, `1.250`, and `10`. `ExactDecimal::parse_sql` plus `rescale_exact(2)` expects coefficients `120`, `0` (non-negative), `125`, and `1000`. `1.251` at scale 2 is an error in that helper and `inexact_decimal` from the CLI.
- TSV covers a null token `\N`, a quoted `\N` that stays a value, decimal(12,2), header absent, and BOM refuse. Unquoted `\N` counts as null. The quoted token counts as value.
- JSONL covers utf8, bool, int64, and `json_text`, including a nested object `{"n":1}`, a JSON null, a missing key, `-0`, and both `i64` extremes. The success document does not contain `{"n":1}`. A fractional `1.0` is `not_int64`.
- Lineage counts match the hand table: CSV value 14, null 1, empty 1; TSV value 9, null 3; JSONL value 17, null 2, missing 1. `batch_rows` is 1, so batches equal rows. An empty file with header absent finishes at 0 rows and the empty SHA-256. The same empty bytes with header exact return `header_required` before a success receipt.
- Hash mismatch (last hex nibble flipped), `TRUE` (`not_bool`), leading zero (`not_int64`), `max_rows` 1, and `bom=refuse` on a BOM file are nonzero and write no result protocol. Stderr for the hash case does not contain `alpha`.
- Unknown format, unknown column type, empty columns, dialect disagreeing with format, a comma null token, protocol `v0`, an extra password field, a JSONL dialect key, and a JSONL `max_field_bytes` key are config failures. The password value is absent from stderr.
- A request of 1048577 bytes is `request_bytes` with a short stderr. A request of exactly 1048576 bytes is parsed and fails as `request_json`. A duplicate `protocol` key is `request_json`. Uppercase hex is `expected_sha256`.
- `cancel_before_read` on a real file is exit 8 fact `cancelled`. A missing path is `read` / `open` even when cancel is also set, and the path token is absent from stderr. A directory is `read` / `not_file`.
- `--help` names the request protocol, CSV, TSV, JSONL, the byte cap, the explicit dialect, and the blocking read, and exits 0 with empty stderr. No arguments, `--request` without a path, `--nope`, `-h`, extra help arguments, and an extra flag are exit 2.
- `oo-pipeline` with no arguments and `import-pg --help` stay exit 2. `catalog-pg extra` is exit 2 and does not invoke the catalog reader. `import-pg` with empty stdin stays exit 1 with `invalid input` and does not print `awaiting_publish`.

## Accepted limits

- The caller names the format. Comma and tab are the only delimited separators, and they come from `format`, then `DelimitedDialect::try_from_separator`. JSONL has no dialect, BOM, header, or null policy in the request.
- Request objects have exact keys. JSON null is rejected because it is not a string, a boolean, or an integer. `expected_sha256` is 64 lowercase hex digits.
- The input file is a local regular file in a trusted context. Symlinks to regular files follow `File::open`. There is no path jail and no path campaign in this command.
- Cancel and the deadline are checked before each blocking `read`, the same limit the readers already document. This command does not interrupt an in-flight `read` and does not claim a worker or an isolation boundary.
- `oo-pipeline` cannot downcast `Decimal128Array` without a new dependency. The decimal proof is the public `ExactDecimal` rescale, the `inexact_decimal` fact, and the profile preimage line `column=amount\tdecimal\t1\t10\t2`. Cell coefficients inside the Arrow batch were not inspected.
- Success is printed only after `finish` returns `Ok`. A pending batch that the reader may deliver before `hash_mismatch` is dropped. `admits_import`, `published`, `arrow_ipc`, and `executes_native` must stay false or the command returns `closed_flag` and no result document.
- Error display from the libraries is not printed. Stderr carries `fact` only.

## Preserved failures

- `linux/preedit` recovered. Offline fetch exit 101 because the private cargo volume had no `chrono-tz` yet. The online locked fetch then exited 0, and the lib and bin smokes passed. The cold-cache log is kept.
- `linux/focused` exit 101: one move of `digest` in `failures_have_no_success_receipt`. Container `1ec26b61746d91d8110f2e7a469e6dea38d22fde449c268d8c9bb4500f428e32`.
- `linux/focused-2`: 11 tests passed. Clippy exit 101 on `needless_range_loop` in `decode_sha256` and `type_complexity` on `pull_delimited` and `pull_jsonl`. Container `a153689f8521e72ea460f14ca5bb1b1177b2c9f11ab18f771490cbf3ad26c99f`.

Those directories, including `console.log` and the exit files, were left as recorded. `linux/focused-3` is the green run.

## Dependency warning left in place

`sqlx-postgres` `message/flush.rs` and `message/mod.rs` emit `unfulfilled_lint_expectations` while the dependency is compiled. Those files are vendor code and were not edited. `cargo clippy -p oo-pipeline --bin validate-file --test validate_file_cli -- -D warnings` exited 0 in `linux/focused-3`. Clippy stderr ends with `Finished dev profile [unoptimized + debuginfo] target(s) in 0.23s`.

## Open work

A separate fixed review of this command has not run. A separate fixed review of the delimited reader has not run. Import, publication, PostgreSQL, HTTP, Arrow IPC, dialect sniffing, and worker isolation stay out of this binary.
