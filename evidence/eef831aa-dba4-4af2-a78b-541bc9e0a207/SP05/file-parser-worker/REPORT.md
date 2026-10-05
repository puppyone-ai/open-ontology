# a207 file parser worker and Arrow IPC 60

UUID `eef831aa-dba4-4af2-a78b-541bc9e0a207`. This slice adds the child binary `oo-file-parser-worker`. It reads one versioned JSON request, opens one caller-admitted regular file, and pulls the public JSONL reader or the public delimited reader. Each record batch leaves as one self-contained Arrow IPC 60 stream inside a length-prefixed frame. The lineage frame for that batch follows. The completion frame is written only after the reader mints its token.

Source commit `6bd2455849da2527973c136968d4549ecd98d770` on parent `d20944017bda07ae8a7e95c8acfcfaebb829aa4a`. Fixed base `a661dcdf72ff91561ae6b7cb79916e582884d08a` is an ancestor of that parent. The commit contains only:

- `crates/oo-connectors/src/bin/oo-file-parser-worker.rs`
- `crates/oo-connectors/tests/file_parser_worker.rs`
- `contracts/connectors/file-parser-worker-v1.md`
- `crates/oo-connectors/Cargo.toml` (the `arrow-ipc` `=60.0.0` pin, default features off, in dependencies and dev-dependencies)
- `Cargo.lock` (four new registry rows)

`source/blobs/` matches that commit. `source/blob-sha256.txt` records the SHA-256 of each blob. No library module, JSONL reader, delimited reader, types, pipeline, gateway, control, core, root manifest, supply-chain policy, or dependency baseline was edited. No parent checkbox, SP05-W02 row, AT-FILE-02 row, or ledger row was changed.

## Behavior

The request is one JSON document on stdin, capped at 1,048,576 bytes, read in 8 KiB chunks. Duplicate keys are `duplicate_key`. Unknown keys are `unknown_key`. The protocol string must be `oo.source.file-parser-worker/v1`. The input class is `local_file` and the admission is `caller_admitted_immutable`. The path is absolute, with no NUL, no trailing slash, and no empty, `.`, or `..` component. The object SHA-256 is 64 lowercase hex digits. The profile names the format. JSONL carries columns only. Delimited carries `dialect` (`csv` or `tsv`), `bom`, `header`, and `null`. There is no dialect sniff.

Credential key names are rejected at any depth. The stderr token is `credentials`, and the key and the value stay off stderr. Because `token` is one of those names, a delimited null marker is `{"policy":"token","text":"..."}`. `text` is the marker passed to `NullPolicy::Token`.

The file check is `symlink_metadata`, then a read-only open with `O_NOFOLLOW`, then `metadata` again. A directory, symlink, or fifo is `not_regular` and is not opened. The process does not canonicalize and does not prove the file immutable.

Stdout is framed binary. Each frame is a little-endian `u32` tag, a little-endian `u32` length, and the payload. Tag 1 is one Arrow IPC stream (schema, one batch, end-of-stream). Tag 2 is the lineage preimage. Tag 3 is the completion receipt. IPC options are alignment 8, legacy false, metadata version V5, compression unset. The IPC cap is `batch_bytes + 65536`. `BoundedWriter` refuses a write that would pass that cap before `extend_from_slice`, and it `reserve_exact`s only the bytes still needed. The length does not pass the cap. The global allocator may still round a reservation up.

The completion frame is written only after `finish` returns, `admits_import` is false, `published` is false, the reader versions match the public constants, and `batches` equals the number of IPC frames. The receipt sets `pg_run_complete=false`, `executes_native=false`, `kernel_network=unsupported`, `kernel_filesystem=unsupported`, `kernel_memory=unsupported`, `parent_kill=caller`, `parent_reap=caller`, and `launcher=not_this_process`. Decimal and int64 values are the Arrow arrays from the public readers. Stderr is `oo-file-parser-worker: <token>` or empty. A panic writes `oo-file-parser-worker: internal` and the process still aborts.

`deadline_ms` is the reader's between-read check. It does not interrupt a blocked `read` and it is not a process kill. The integration test is the parent: it clears the child environment, reads bounded frames, and kills and reaps the child on timeout or panic. The binary does not install a watchdog, seccomp, landlock, or an rlimit.

The IPC ceiling test uses 64 decimal columns and 128 rows. Column memory is above `batch_bytes + 65536`, so the child exits 8 with zero frames. The public reader has already allocated that `RecordBatch`. The cap binds the IPC sink.

## Linux Rust 1.94

Image `docker.io/library/rust@sha256:0f24958fa693fe838859faf64a0ec78d382cd543a6b6efd0fa7a3bcb10ea90d5`, `linux/arm64`, Rust `1.94.0` (`4a4ef493e 2026-03-02`). `RUSTUP_TOOLCHAIN=1.94.0`. Private network `oo-a207-a207preedit01-net`, cargo volume `oo-a207-a207preedit01-cargo`, target volume `oo-a207-a207preedit01-target`. Cap drop `ALL`, no new privileges, memory 8g, cpus 4, pids 4096, no host network, no published ports, tmpfs `/tmp` `rw,nosuid,nodev,size=1g`, `HOME=/tmp`, private `CARGO_HOME` and `CARGO_TARGET_DIR`. No PostgreSQL. `CLEANUP.txt` records removal of that network, those volumes, and the task containers after the logs were stored. The pinned image remains. `docker prune` was not run.

`build08/capacity.txt` at launch: `docker_rust_builders=0`, `host_cargo=0`, `cap=10`.

| Run | Result |
|---|---|
| `preedit` | Exit 0. Container `177459da028c`. `--test delimited` 25 passed. `--test jsonl_reader` 11 passed. Snapshot of `e2da75f50d7b3e0f56b4c3d2e64a87e8e0d950ab` before this slice. |
| `build` | Exit 101. Container `1053c811ff8c`. `cargo update -p arrow-ipc` cannot see a crate that is not in the lock. `Cargo.lock` was not modified. Log preserved. |
| `build02` | Exit 1. Container `1d075ef0f31d`. Cargo 1.94 `cargo fetch` has no `-p`. Log preserved. |
| `build03` | Exit 4. Container `b4d27d31e655`. `cargo fetch` added the four crates. The gate required flatbuffers `25.2.10` and the resolver selected `25.12.19`. No existing checksum changed. Log preserved. |
| `build04` | Exit 101. Container `d12f893b1238`. Format strings used field access, which Rust 1.94 rejects. Test compile exit 101. Log preserved. |
| `build05` | Exit 101. Container `16d96d36e6ec`. `CARGO_BIN_EXE_oo_file_parser_worker` is unset. This Cargo sets the hyphenated name. Log preserved. |
| `build06` | Exit 101. Container `fd5fb969f4da`. 11 tests passed. `csv_round_trip` exited 2 because the null-marker key `token` is also a credential key. Log preserved. |
| `build07` | Test exit 0, 12 passed. Clippy exit 101 on `let_unit_value` and two `while_let_loop` hits. Container `5ffa4cd5c673`. Log preserved. |
| `build08` | Exit 0. Container `eec3c01ff134`. `OOMKilled=false`, `Privileged=false`. `--test file_parser_worker` 12 passed in 0.41s. Clippy `-D warnings` on the bin and that test exited 0. `cargo-fetch.exit` 0. `lock-gate.exit` 0. |

Dependency compile printed two pre-existing `sqlx-postgres` `unfulfilled_lint_expectations` warnings. Those files were not edited. The oo-connectors clippy command still exited 0.

## Dependency closure

`arrow-ipc` 60.0.0 is the crates.io release created `2026-09-15T15:30:28Z`, license Apache-2.0, `rust-version` 1.88, yanked false, checksum `86c523472d22e31f984df7a44fc36e6323d3c9b05ffe53d267ee4c3aa7c128d2`. At the build08 clock `2026-10-05T20:17:21Z` the age floor is 20 days. The seven-day gate passes. `arrow-select` 60.0.0 (`2026-09-15T15:30:11Z`, checksum `ba3482b7d03bf4a3bf0b195f08a7f2b071595ccd54897e35b975ac63d3104977`) and `arrow-cmp` 60.0.0 (`2026-09-15T15:30:07Z`, checksum `73a0e9adb28d95739fabcdae637261a2c14e618caf58ba1b88857120f9ee2687`) are the same series, the same license, and the same age floor.

`arrow-ipc` depends on `flatbuffers` `version = "25.2.10"` with default features off, which Cargo treats as `^25.2.10`. The resolver selected `25.12.19`, created `2025-12-19T23:16:16Z`, license Apache-2.0, `rust_version` 1.51, yanked false, checksum `35f6839d7b3b98adde531effaf34f0c2badc6f4735d26fe74709d8e513a96ef3`. Age floor 289 days. The seven-day gate passes. Its normal dependency `bitflags` `^2.8.0` stayed on the already locked `2.13.2`. Its build dependency `rustc_version` `^0.4.0` stayed on `0.4.1`. The optional `serde` feature is off. `lz4_flex` and `zstd` are not in the lock diff.

The lock diff against the parent is four added packages and zero checksum changes, removals, or version moves. `supply-chain/policy.md` and `dependency-baseline.json` were not edited. The new rows are not marked admitted. No seven-day exception was requested or granted.

Crates.io JSON for these versions is under `oracles/`.

## Hashes

The product object hash is `sha2` inside the worker. The integration test hashes fixtures with `/usr/bin/sha256sum` and checks the empty digest `e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855` in `sha256sum_matches_the_empty_vector`.

| Blob | SHA-256 |
|---|---|
| `Cargo.lock` | `16e6afda29ae4bae251d569f3c4ad16459527f0d0323598d8e4e0971818d458a` |
| contract | `f544a6bb37c0012613d02c7a335477e8da470fcf0c24780b8d1b8b2dcba54550` |
| `Cargo.toml` | `e0c0466eb75c3de8acf4210bdc5a2ff5ccb9358a93ce19bb6abeca4964afbfd6` |
| worker | `de5ad3f3c7bcf9368011aff422a4aa24043794231a49bde5d8679b0cf7acf0e0` |
| test | `6a231e716c0baac0cf03360b15433fa209a93f756a110b35ff06432ba91f60bd` |

`MANIFEST.sha256` is the SHA-256 of every file in this evidence directory except itself, computed with Python `hashlib` on the host after the tree was complete. No second Rust builder was started for that manifest.

## What this slice does not do

It does not import, publish, or finish a PostgreSQL run. `admits_import` and `published` stay false. The receipt is a parse proof. Kernel network, filesystem, and memory restriction stay `unsupported`. The parent owns the kill and the reap. SP05-W02, AT-FILE-02, and the phase ledger stay open. Independent review of `6bd2455849da2527973c136968d4549ecd98d770` has not run.
