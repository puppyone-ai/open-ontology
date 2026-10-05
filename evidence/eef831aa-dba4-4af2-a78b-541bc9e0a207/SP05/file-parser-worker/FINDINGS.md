# Findings

Reviewed the owned worker, the contract, the lock diff, and the Linux logs for `6bd2455849da2527973c136968d4549ecd98d770`.

## Accepted limits

- The child is a real process. The test spawns `CARGO_BIN_EXE_oo-file-parser-worker` with an empty environment, a fresh regular file, and pipes. It decodes each IPC payload with `arrow_ipc::reader::StreamReader`. It does not call the readers in-process as a stand-in for the worker.
- Request JSON is capped at 1 MiB and parsed with `oo_core::parse_json_value`, so duplicate keys fail before a profile is built. Integers are lexical. `deadline_ms` is `1..=3600000` and becomes `Instant::now()` plus that duration when the request is accepted.
- The path check refuses relative paths, trailing slashes, and `.` or `..` components. Open uses `symlink_metadata` and `O_NOFOLLOW`. A fifo is `not_regular` and is not opened. Replacing one regular file with another regular file between the two checks is not detected. The process does not jail the path and does not prove immutability.
- Credential key names, including `token`, are rejected anywhere in the tree. The delimited null marker therefore uses the key `text`. The policy value remains `token`, and `NullPolicy::Token` still receives that string. `\N` is a legal marker under the public reader rules.
- JSONL requests that carry dialect, BOM, header, or null keys are `unknown_key`. Delimited requests must name those fields. There is no sniff.
- One IPC frame is one stream: schema, one batch, end-of-stream. Alignment is 8, metadata version is V5, legacy is false, and compression is unset. `lz4_flex` and `zstd` stay out of the closure.
- The IPC cap is `batch_bytes + 65536`. The memory sum of the batch columns is checked before `StreamWriter`. `BoundedWriter` returns before `extend_from_slice` when the length would pass the cap. The 64-by-128 decimal case exits 8 with zero frames. The public reader still allocates that `RecordBatch` before the check. The ceiling is an IPC-sink ceiling.
- Decimal coefficients and int64 values are the Arrow arrays. The CSV fixture checks `i64::MIN`, `i64::MAX`, `1.20` and `1.2` as coefficient 120, UTF-8 `中文` and `猫`, bool, null, a quoted empty field, and a quoted newline. The TSV fixture checks a quoted tab, an unquoted comma, and `-0` stored as 0. JSONL checks null against a missing key, and the missing-key offset is the `{`.
- The completion receipt is written only after `finish`. `admits_import=false` and `published=false`. `pg_run_complete=false`. A hash mismatch can emit the batch and then exit 6 with no complete frame. Truncation, a record budget, a BOM refusal, and a bad request omit the complete frame. Empty input with `header=absent` completes with zero rows. Empty input with `header=exact` is `header_required` before the hash. A header-only file with `header=exact` completes with zero data rows.
- `deadline_ms` does not kill a blocked read. The parent test holds stdin open, kills the child, and checks `/proc/<pid>` is gone. The drop guard does the same if the test panics. Exit status of a signal has no code.
- Stderr is one static token. The credential test and the hash-mismatch test check that the path and the cell text are absent.
- The new lock rows are `arrow-ipc` 60.0.0, `arrow-select` 60.0.0, `arrow-cmp` 60.0.0, and `flatbuffers` 25.12.19. Existing checksums are unchanged. Age floors at the build08 clock are 20 days for the Arrow crates and 289 days for flatbuffers. All four are Apache-2.0 and unyanked. The baseline file was not edited and the rows are not admitted.

## Preserved failures

- `build` exit 101: `cargo update -p arrow-ipc --precise 60.0.0` does not match a package that is only in `Cargo.toml`. Container `1053c811ff8c`. The lock on disk did not change.
- `build02` exit 1: `cargo fetch -p oo-connectors` is rejected by Cargo 1.94. Container `1d075ef0f31d`.
- `build03` exit 4: the first lock gate required flatbuffers 25.2.10. `cargo fetch` selected 25.12.19 under `^25.2.10`. Container `b4d27d31e655`. The later gate accepts that resolved version. The policy file was not weakened.
- `build04` exit 101: `{identity.protocol}` is not a valid format string on Rust 1.94. Container `d12f893b1238`.
- `build05` exit 101: the underscore `CARGO_BIN_EXE` name is unset. Container `16d96d36e6ec`.
- `build06` exit 101: 11 passed, and the CSV round trip exited 2 on the credential key `token`. Container `fd5fb969f4da`.
- `build07`: 12 passed. Clippy rejected `let _ = set_hook(...)` and the two `loop { match next_batch }` shapes. Container `5ffa4cd5c673`.

Those directories, including `console.log` and the exit files, were left as recorded.

## Dependency warning left in place

`sqlx-postgres` `message/flush.rs` and `message/mod.rs` emit `unfulfilled_lint_expectations` while the dependency is compiled. Those files are vendor code and were not edited. `cargo clippy --locked --offline -p oo-connectors --bin oo-file-parser-worker --test file_parser_worker -- -D warnings` exited 0 in `build08` after that warning text. Clippy ended with `Finished dev profile [unoptimized + debuginfo] target(s) in 0.23s`.

## Open work

A separate fixed review of `6bd2455849da2527973c136968d4549ecd98d770` has not run. SP05-W02, AT-FILE-02, and the phase ledger stay unchecked. This binary is not a restricted worker, a loader, or a public endpoint. Kernel enforcement, a parent harness outside the test, and publication stay open.
