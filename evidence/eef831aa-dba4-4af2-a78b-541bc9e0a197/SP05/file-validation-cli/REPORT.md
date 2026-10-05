# a197 validate-file command

UUID `eef831aa-dba4-4af2-a78b-541bc9e0a197`. This slice adds the `validate-file` binary to `oo-pipeline`. The command reads one `oo.pipeline.validate-file/v1` request file, builds a CSV, TSV, or JSONL reader through the public constructors, pulls a real `std::fs::File` until a clean EOF, and writes one `oo.pipeline.validate-file-result/v1` summary only after `finish` returns a receipt.

Source commit `59be14baab1d5f30abaa2ce8cf67d607a2e4417f` on parent `ef6c4fc7da03342f900146e90ff692fe742d1ba4`. The commit contains only:

- `crates/oo-pipeline/src/bin/validate-file.rs`
- `crates/oo-pipeline/tests/validate_file_cli.rs`
- `contracts/pipeline/validate-file-cli-v1.md`

`source/blobs/` matches that commit. `source/blob-sha256.txt` records the SHA-256 of each blob. No manifest, lock, library module, connector, control, gateway, or import source was edited. No parent checkbox was changed. No release is claimed.

The delimited reader in `671507b229ad7d1bc6a2e948b5b74d6d7252393f` is the author commit this command calls. This task does not accept that library. A review of this command still has to check the integration. It does not close the reader review.

## Behavior

The process accepts `validate-file --help` or `validate-file --request <path>`. Any other argument list exits 2 with one `oo.pipeline.validate-file-error/v1` object on stderr and an empty stdout. Help text exits 0 on stdout.

The request is capped at 1048576 bytes from metadata before the body buffer is filled, then read with `take(1048577)`. `oo_core::parse_json_value` rejects duplicate keys, excess depth, excess nodes, and trailing bytes. Unknown keys, including a password field, are `unknown_field`. The value is not copied into the diagnostic. `format` is `csv`, `tsv`, or `jsonl`. The dialect byte comes from that format and must match `profile.dialect` for CSV and TSV. JSONL rejects a dialect key. There is no sniff.

CSV and TSV go through `DelimitedDialect::try_from_separator`, `DelimitedColumn::try_new`, `DelimitedProfile::try_new`, and `DelimitedBudget::try_new`. JSONL goes through `JsonlColumn::try_new`, `JsonlProfile::try_new`, and `JsonlBudget::try_new`. `queue_batches` is not a request field. The reader keeps it at 1. `deadline_ms` is an integer from 1 to 120000, converted to `Instant::now() + Duration` before the input file is opened.

The input path is a trusted local regular file. The command does not fetch HTTP, does not authenticate a public caller, and does not echo the path. A missing file is outcome `read` / `open` before a cancel flag is observed. `cancel_before_read` sets the reader cancel handle after `open` and before `next_batch`.

Success stdout is one sorted JSON object plus a newline: protocol, source protocol, profile and receipt versions, profile id, profile digest, config identity, object SHA-256, rows, columns, batches, types, and a lineage fold. `admits_import` and `published` come from the receipt and are false. `arrow_ipc` and `executes_native` come from the reader header and are false. The document has no row payloads, no cell text, and no database credentials. A hash mismatch, a parse error, a budget error, or a cancel writes no success document.

Exit codes: 0 success, 2 usage, 3 config, 4 read, 5 parse, 6 budget, 7 hash, 8 cancel. Stderr facts are the static library fact or a closed token.

## Linux Rust 1.94

Image `docker.io/library/rust@sha256:0f24958fa693fe838859faf64a0ec78d382cd543a6b6efd0fa7a3bcb10ea90d5`, `linux/arm64`, Rust `1.94.0` (`4a4ef493e 2026-03-02`), host `aarch64-unknown-linux-gnu`. `RUSTUP_TOOLCHAIN=1.94.0` overrides `rust-toolchain.toml` channel `1.98.1`. Private network `oo-a197-eef831aa-net`, cargo volume `oo-a197-eef831aa-cargo`, target volume `oo-a197-eef831aa-target`. Cap drop `ALL`, no new privileges, memory 8g, no host network, no published ports, tmpfs `/tmp` `rw,nosuid,nodev,noexec,size=1g`. No PostgreSQL. One Rust builder at a time. `CLEANUP.txt` records removal of that network, those volumes, and `/tmp/oo-a197-eef831aa-dba4-4af2-a78b-541bc9e0a197` after the logs were stored. The pinned image remains. `docker prune` was not run. Foreign containers were left in place.

| Run | Result |
|---|---|
| `linux/preedit` | Exit 0. Container `7e47ca4902a4e1a02ebae71b15ca18420b294d9bd40442abbbdc319176fcc7a5`. Archive of `9d658725fabfbf38694227bf462ba23d9c4d39cb` without the three new files. `cargo test -p oo-pipeline --lib` 11 passed. `cargo test -p oo-pipeline --bins` 0 passed. `absence.exit` 0. Offline fetch exit 101 (`chrono-tz` absent from a cold cargo volume), then online `cargo fetch --locked` exit 0. `OOMKilled=false`, `Privileged=false`. |
| `linux/fmt` | Exit 0. `rustfmt 1.8.0-stable` rewrote the two Rust files. No cargo build. |
| `linux/focused` | Exit 101. Container `1ec26b61746d91d8110f2e7a469e6dea38d22fde449c268d8c9bb4500f428e32`. Archive hashes matched. The test did not compile: `digest` was moved by `into_bytes` and reused. Log preserved. |
| `linux/focused-2` | Exit 101. Container `a153689f8521e72ea460f14ca5bb1b1177b2c9f11ab18f771490cbf3ad26c99f`. `--test validate_file_cli` 11 passed. Clippy `-D warnings` rejected `needless_range_loop` in the binary and `type_complexity` on the two pull helpers. Log preserved. |
| `linux/fmt-2` | Exit 0. rustfmt after those two fixes. |
| `linux/focused-3` | Exit 0. Container `909c3f9f169218010d9c265bcbcfaff1a7912c9d905a53ce8bf488ff2bc7605d`. Capacity `own=0 global=1 host_cargo=0 cap_global=6`. Archive base `ef6c4fc7da03342f900146e90ff692fe742d1ba4` plus the three files. Offline fetch exit 0. `--test validate_file_cli` 11 passed in 0.01s. Clippy on `--bin validate-file --test validate_file_cli` exit 0 (`Finished dev profile` in 0.23s). `rustfmt --check` exit 0. `worst_phase_exit=0`. |

The focused-3 bytes are the source commit. `sha256sum` of the worktree files matched `linux/focused-3/expect-sha256.txt` before the commit, and `source/blobs/` matches the commit.

`sqlx-postgres` still prints two `unfulfilled_lint_expectations` warnings from vendor code. Those files were not edited. The clippy command exited 0 after that text. Clippy stdout is empty.

## Hashes

The product object hash is the reader’s `sha2` digest, copied from the finish receipt. The integration test compares that hex, the profile digest, and the config identity to a separate safe-Rust SHA-256 in `tests/validate_file_cli.rs`. That oracle self-checks empty (`e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855`) and `abc` (`ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad`). Focused-3 wrote the dumps under `linux/focused-3/oracle/`. The same bytes are in `oracle/`.

| Object | SHA-256 |
|---|---|
| positive CSV | `a3af7afa9f7ee523cf409f47039eeb792f4e0099a775461618e85dbd28fb2650` |
| positive TSV | `2fc7a104e5b8dd93f84ca61d5b87f11908c273e84fb59c21af47cf7f5bce7da4` |
| positive JSONL | `ab2a6d2af7831fdd9d0dfa6f4bf96a969646340fd64af03eee6bd75b43e52886` |
| CSV profile | `b1ded4170ec6ac0b925f5fb31a2333cfdd19704013b5b2b108a610bcd7ae4394` |
| CSV config | `cba04de1d6ad14af5440333de050f7f4ebee79d08b36d03adef74940eab2e186` |
| TSV profile | `b1a382e46591100d112065110f6181b22b5784d0dcbde41aa823bbeac5c30d4d` |
| TSV config | `65a7a1f4f03d336b4d327cb2af758e5e45be7ff34de20c8ef905d5d99b9fcc36` |
| JSONL profile | `fc1726ca51c759b3ed8d3e131895825c86eebec9531a6d4bd3c47825fc399aa8` |
| JSONL config | `647caacc27aa9efc95fb03107c8b8d1002fef0613370601f06c3005f63230ffe` |

CSV success: 4 rows, 4 batches, lineage value 14, null 1, empty 1, missing 0, lines 2 through 5, decimal column precision 10 scale 2. TSV success: 3 rows, value 9, null 3, empty 0, decimal precision 12 scale 2. JSONL success: 5 rows, value 17, null 2, missing 1, empty 0, payload type `json_text`. All three documents have `admits_import`, `published`, `arrow_ipc`, and `executes_native` false.

Launch scripts use `sha256sum` only to check the readonly archive. That check is archive identity. `MANIFEST.sha256` is the SHA-256 of every file in this evidence directory except itself, computed with Python `hashlib` on the host. No second Rust builder was started for that manifest.

## What this slice does not do

It does not import rows, publish a table, open PostgreSQL, speak HTTP, or write Arrow IPC. The summary is a parse receipt. Decimal cell bytes were not read from `Decimal128Array`: `oo-pipeline` has no direct `arrow-array` dependency, and this task does not change a manifest. Exactness is checked with `oo_core::ExactDecimal::parse_sql` and `rescale_exact` in the test, with `inexact_decimal` on `1.251` at scale 2, and with a profile preimage that names `decimal`. A fixed review of this command has not run. A fixed review of the delimited reader has not run.
