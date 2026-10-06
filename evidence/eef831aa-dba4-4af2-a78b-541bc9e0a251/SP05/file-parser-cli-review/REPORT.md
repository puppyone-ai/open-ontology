# Report

Verdict: **APPROVE_LIMITED**.

UUID `eef831aa-dba4-4af2-a78b-541bc9e0a251`. Reviewed source `0a30fcda7fb586bd8d6955e03ae306888374b915`, tree `ee69dec5a05f66f136e2e7cd72ac0bce65fd918d`, parent `9907d40bc0d2fd684a786cd8f98473d52403fc3e`. Date of this review: 2026-10-06.

The approval covers the `parse-file` command at that commit: one explicit CSV, TSV, or JSONL profile, a bounded request, and a success document only after `ParentPull::next_batch` returns `Ok(None)` and `finish` returns `ParentParseComplete`. `admits_import`, `published`, and `pg_run_complete` stay false. `kernel_restriction` is `unsupported`. That string is the public constant. It is not a containment certificate.

This review does not close import, publication, PostgreSQL, HTTP, a public authentication decision, dialect sniffing, GB18030, Parquet, Excel, the worker review, or the parent review. SP05-W02, AT-FILE-02, and the phase ledger stay open. `governance/review-guidance.md` says a partial slice does not mark the parent acceptance case passed.

Author evidence is `evidence/eef831aa-dba4-4af2-a78b-541bc9e0a245/SP05/file-parser-cli/`. Its handoff names command `0a30fcd`, contract `9907d40`, and author archive `1ddbd865fc77c7a1a6209949336f592107d77683`. That archive is the author's build input. This review compiled a fresh `git archive` of `0a30fcd`. The author record is not this approval.

No product file, manifest, lock, contract, or ledger was edited. This directory is the review archive.

## What was read

- `evidence/eef831aa-dba4-4af2-a78b-541bc9e0a245/SP05/file-parser-cli/REPORT.md`, `FINDINGS.md`, and `HANDOFF.md`
- `contracts/connectors/parse-file-cli-v1.md` at the fixed commit, saved as `source/blobs/parse-file-cli-v1.md`
- `source/blobs/parse-file.rs` and `source/blobs/parse_file_cli.rs`
- The a204 file-validation review launch and report shape, the a200 proof launch hash, and the a199 manifest tool. Origins and hashes are in `recipes/ORIGIN.md`
- `governance/review-guidance.md`
- Plan SP05-W02 and the open F-D03 checkbox. They were not edited

Repo HEAD when `linux/preoverlay` started was `9d430062f5644a635f8293717dfc118c3ba5408b`. Repo HEAD when `linux/overlay-2` started was `0ba54ca5996e4a5ff8dc9052ea10a434f839a537`. At the time of this report, HEAD is `92157f5c2bce88614ea0068ca2cc8949261c2727`. `git diff 0a30fcda7fb586bd8d6955e03ae306888374b915 HEAD` is empty for `parse-file.rs`, `parse_file_cli.rs`, `parse-file-cli-v1.md`, `oo-file-parser-worker.rs`, `file_worker_parent.rs`, `crates/oo-connectors/Cargo.toml`, the root `Cargo.toml`, and `Cargo.lock`. The process that was tested is the sealed archive, not a later worktree.

## Pre-overlay smoke

`linux/preoverlay` ran before any review harness existed. The archive check refused `crates/oo-connectors/tests/parse_file_a251_review.rs`. `blob-cmp.exit` 0. `overlay-absent.exit` 0.

Rust `1.94.0` (`4a4ef493e 2026-03-02`), host `aarch64-unknown-linux-gnu`, cargo `1.94.0`, Debian 13, image `docker.io/library/rust@sha256:0f24958fa693fe838859faf64a0ec78d382cd543a6b6efd0fa7a3bcb10ea90d5`. `RUSTUP_TOOLCHAIN=1.94.0` overrides `rust-toolchain.toml` channel `1.98.1`. Container `fa515ca94e48de15b9fad0c4253e97f4c442c6bf9d0ffd4dab1619dae5c7e6d8`. Memory `6442450944`. `NanoCpus=1000000000`. `CapDrop=["ALL"]`. `OOMKilled=false`. `Privileged=false`. Network `oo-a251-eef831aa-net`. `/src` was read-only. `CARGO_BUILD_JOBS=1`.

`fetch-offline.exit` 101 is the cold private cargo volume (`chrono-tz` absent). The same script then ran `cargo fetch --locked` and recorded `fetch.exit` 0. `test.exit` 0 is the existing `parse_file_cli` suite: `ok. 12 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 205.59s`. `clippy-component.exit` 0. `clippy.exit` 0 on `--bin parse-file --test parse_file_cli -- -D warnings`. `docker-run.exit` 0. Two `sqlx-postgres` `unfulfilled_lint_expectations` warnings are in `clippy.stderr` (`flush.rs:11`, `message/mod.rs:76`). They are the locked vendor. Clippy still exited 0. Stderr ends `Finished dev profile [unoptimized + debuginfo] target(s) in 29.46s`.

## Decisive integration run

`linux/overlay-2` copied `overlay/parse_file_a251_review.rs` onto a fresh archive of `0a30fcd` as `crates/oo-connectors/tests/parse_file_a251_review.rs`. The product blobs match `source/blobs`. `blob-cmp.exit` 0. `fetch-offline.exit` 0. `fetch.exit` 0. `clippy-component.exit` 0. `test.exit` 0. `hash-compare.exit` 0. `clippy.exit` 0. `docker-run.exit` 0.

`test.stdout`: `ok. 12 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 224.09s`.

Container `4e9cc7cee1c1096e7193c4de3c2cdbe4a041ea7cc0439a0f25ccad8e817c9b67`. Same image, memory, CPU, cap drop, and privilege settings as the preoverlay run. `tmpfs /tmp` is `rw,nosuid,nodev,noexec,size=1g`. Mounts bind the archive at `/src` with `rw=false`. The harness was not present in the preoverlay archive. `linux/overlay-2/expect-review.rs` matches `overlay/parse_file_a251_review.rs` (`0564759838c6cc7dffa52c0b6eb90d3c131d62fbfd7a2067ca818cf643358365`).

The harness spawns the compiled `parse-file` binary. It also calls `ParentConfig::start` on the same production `oo-file-parser-worker`, folds lineage from `VerifiedBatch` (`line`, `byte_offset`, `raw_bytes`, `arrow_bytes`, presence counts), and compares that fold to the CLI JSON. It does not read Arrow cell values. Handwritten presence counts are a second check. Object and lineage digests are the harness SHA-256, which self-checks the empty digest and `abc`. `sha256sum` of the saved fixtures and of the lineage preimages equals those Rust digests (`linux/overlay-2/hash-compare.txt`). The harness does not link `sha2`. `OO_A251_ORACLE_DIR` receives the fixtures. No PostgreSQL client or server is started.

Clippy is `cargo clippy --locked --offline -p oo-connectors --test parse_file_a251_review -- -D warnings`. `clippy.stdout` is empty. Stderr ends `Finished dev profile [unoptimized + debuginfo] target(s) in 0.29s` after the same two vendor warnings. The test profile finished in 3.02s. The existing `parse_file_cli` suite was the preoverlay run. It was not repeated in this container.

## Fresh receipts

Success stdout is one `oo.connectors.parse-file-result/v1` object. The documents are `linux/overlay-2/oracle/csv.stdout.json`, `tsv.stdout.json`, `jsonl.stdout.json`, and `empty.stdout.json`. Each has `admits_import`, `published`, and `pg_run_complete` false, `kernel_restriction` `unsupported`, and `parent_protocol` `oo.source.file-worker-parent/v1`. `arrow_ipc` is true only when `batches` is greater than 0. The empty file has `arrow_ipc` false and `arrow_bytes` 0.

The harness rejected `PATHTOKEN_a251` and the worker path on stdout and stderr. It also rejected `CELL_a251`, `SECRET_a251`, and `1.20` on the CSV run, `甲` and `-3` on the TSV run, and `丙` on the JSONL run.

| Fixture | Rows | Batches | Columns | Value | Null | Empty | Missing | Object SHA-256 |
|---|---:|---:|---:|---:|---:|---:|---:|---|
| CSV `a251csv` | 4 | 2 | 4 | 12 | 3 | 1 | 0 | `a82155e70d4ccd1787149cde689b4b9acdf9e911e16da7f3fd15dd8a3c746cc1` |
| TSV `a251tsv` | 2 | 2 | 2 | 3 | 1 | 0 | 0 | `17765af692fdf1f83732808353e7866dc6bca2c2212815512e5dcdd689b2dd1c` |
| JSONL `a251jsonl` | 3 | 2 | 3 | 7 | 1 | 0 | 1 | `f31e2f56091c6936ebf31cfec1d8426ee7080046b8f8b84241d6c04e15cd3244` |
| Empty JSONL `a251empty` | 0 | 0 | 1 | 0 | 0 | 0 | 0 | `e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855` |

CSV profile digest `23c19f3edefc04a28f325b7e31ee8a8cf666d51e06433e78a9c2259777fdd1a3`, config identity `8f782e9a3f4904f9eea0a52ab42d74b415b92ce80cff120dcb0f5f1212b0c4c8`, schema `1be80987e132338c5a958644511439d59892484f9890d348dae04670df088c21`. Reader `oo.source.delimited/v1`. Lineage first line 2 at offset 18, last line 5 at offset 66. `raw_bytes` 70. `arrow_bytes` 3288. Price type is decimal precision 10 scale 2. Lineage SHA-256 `3b65357f4bd96e455e4e0b228a30e0eaf710d17656fb47dc4201c83d203de940`.

TSV profile digest `9a5adcdf0873c7f0a1d194a32d9b101129af2dfefb9982367b7dd1ed6d0b42fa`, config identity `73c89fc5017d1f56ec0c343a3c0db43f8d09340225f25b7ed0405047099e95f9`, schema `a7a4d5237b3037eb2aae317c084f515a978391c6df4de2556c02599d0219c368`. Lines 2 and 3, offsets 7 and 14. `raw_bytes` 10. `arrow_bytes` 2576. Lineage SHA-256 `144905ea36dacc30c0ff1febb6c1e814938af637c35560168cc6d4af9d9244a7`.

JSONL profile digest `78e76416625e0253a70b81c95f85bde036eed9c3b450db17d6dbb047a5a7f56a`, config identity `fe75312e5a048f6e91c06604e497c6cff5fb6c9e448c4ab8f87693d120b72520`, schema `4f8b3cdd04bc0bcc4650aa79b9a527570aebfcd8cfa47c63754deb470861e5fb`. Reader `oo.source.jsonl/v1`. Lines 1 and 3, offsets 0 and 62. `raw_bytes` 77. `arrow_bytes` 2956. Lineage SHA-256 `2f48cfef1d3d90e3584b81b5fb7d7942539172ad7bb71fa9538942e10eb8e7e6`.

Empty profile digest `83a150335d40ee2fd265060a0f287ed0a2c7b2ba3d686f5dd4422abacae713e8`, config identity `b2d0f55e56dca39f6bd0b1dd000a436e8867a29191fc05a8d4e9d3523a012c54`, schema `c39f066dd87cfd164a869225dd491d7f8184fd059b2a04fd2260ebf380398928`. Every lineage counter is 0. Lineage SHA-256 `54a5c7e9165ede96bd88fbdfc37992005d04042b15d5c487cc537cdc1e9b1522`.

Schema, profile, and object identities on stdout matched a fresh `ParentParseComplete` from the same worker. The lineage preimage is the contract text: the prefix `oo.connectors.parse-file-lineage/v1`, decimal counters, and a trailing newline on every line, including `raw_bytes` and `arrow_bytes`. The saved CSV preimage has no cell text.

## Failures leave no success receipt

Observed on the compiled binary. Stdout was empty. Stderr was one `oo.connectors.parse-file-error/v1` object.

| Case | Exit | Outcome | Fact | Worker token | Reaped |
|---|---:|---|---|---|---|
| Flipped executable digest | 6 | `hash` | `executable_sha256` | null | false |
| Flipped input digest | 6 | `hash` | `hash_mismatch` | `hash_mismatch` | true |
| Malformed delimited quote | 5 | `parent` | `ended` | `protocol` | true |
| Exact header on an empty file | 5 | `parent` | `ended` | `header_required` | true |
| Extra `password` key | 3 | `config` | `credentials` | null | false |
| Relative worker path | 3 | `config` | `worker_path` | null | false |
| Parent deadline on 40000 JSONL rows | 7 | `deadline` | `timed_out` | null | true |

The executable-hash document has `group_signal` `not_needed`, `exit_code` null, and `term_signal` null. The child exit on the input-hash document is 6. The malformed quote and the exact-header documents report child exit 4. A separate `ParentConfig::start` on those same bytes returned `FailureKind::HashMismatch`, `FailureKind::Ended` with token `protocol`, and `FailureKind::Ended` with token `header_required`.

The credential run omitted `SECRET_a251_CRED` and the key name `password`. The fact string is `credentials`. The relative path `PATHTOKEN_a251/worker` is absent from that error document. `group_signal` on these refusals is `not_needed`.

`--help` exits 0, writes the usage text to stdout, and leaves stderr empty. The observed text contains `oo.connectors.parse-file/v1`, `1048576`, CSV, TSV, JSONL, `does not infer`, `does not import`, `does not publish`, `does not open a database`, `ParentParseComplete`, `no raw rows`, `clean EOF`, `unsupported`, and `parent_deadline_ms`. `--help --extra` is exit 2, outcome `usage`, fact `unknown_flag`.

## Deadline, then the next call

`z_deadline_kills_then_the_next_call_completes` uses 40000 JSONL rows of `{"n":1}\n` and `batch_rows` 16. An empty JSONL parse took 8929 ms. The full parse took 74438 ms and returned a closed summary. The kill deadline is half the gap, 32754 ms, which is inside 1..=120000 and longer than the empty parse. The killed call returned in 41599 ms, shorter than the full parse.

`linux/overlay-2/oracle/deadline.stderr.json`: `exit_code` null, `fact` `timed_out`, `group_signal` `sent`, `outcome` `deadline`, `reaped` true, `term_signal` 9, `worker_token` null. `deadline.txt` records `teardown=signal9`. The harness also accepts a null `term_signal` with child exit 7. This run took the signal 9 branch. `linux/overlay` took that same branch (`term_signal=Some(9)`, `kill_elapsed_ms=41030`). One observation is not a guarantee that the closed-pipe branch cannot happen. `group_signal` `sent` means the parent reported that `kill` returned success. It is not a certificate that every descendant died.

The next call is a new process. It parsed the empty JSONL file, matched a fresh parent receipt, and printed `deadline_next=empty_ok`.

## Preserved attempt

`linux/overlay` is the same 12 tests on the earlier harness. `test.exit` 0 (`finished in 223.32s`). `hash-compare.exit` 0. Clippy `-D warnings` rejected `document` for `too_many_arguments` (8/7) at `parse_file_a251_review.rs:518`. `clippy.exit` 101. `docker-run.exit` 101. Container `e6eed3cccf5682ffdcd4c20e015b98c8b5b257c06593103572e34f6324daa624`. The vendor warnings printed first. The product sources in that directory match the fixed commit. The harness change after that failure wraps the executable digest and path in `ExeOverride` and passes `Hand` by reference. It is only in `overlay/parse_file_a251_review.rs`. The product tree was not edited. `linux/overlay/expect-review.rs` is the pre-fix copy (`43c17acf2dca60afab692af183b52594b2a42a8bc28b4fa718790d4a7cdd854c`).

## Limits

The input is a caller-admitted local path. The command does not open that file itself. A wrong input digest still starts the worker, and that call cannot return `ParentParseComplete`. Hashing the worker and the later `start` are not one snapshot. This review did not prove the executable stays unchanged between those two steps.

`parent_deadline_ms` is the parent clock, 1..=120000. `budget.deadline_ms` is the worker between-read clock. The two clocks are not added. The executable hash has its own 120000 ms clock and an 8192-byte read, capped at 2147483648 bytes. This review did not spend that hash clock.

The deadline fixture is one bounded JSONL file. It is not a kernel campaign. No seccomp, landlock, `setsid`, or rlimit probe was added. PostgreSQL was not started. The connector manifest and the root lock were not edited. a207 remains the sole writer of those files for connector dependency changes. This task made no dependency change.

Proof is `linux/arm64`, Rust 1.94.0, one builder at 6 GiB and 1 CPU. `MANIFEST.sha256` is the std-only Rust inventory of this directory.

## Sealed blob digests

`sha256sum` inside the overlay-2 container:

```text
98e56bec8a5b5da33233444cd24e2e7aba48b89040475ec104be8dd72e3ee246  crates/oo-connectors/src/bin/parse-file.rs
ddb3908ac5038cad467dcd73b1954b247d505f9e58a0e4badcca40af223e952a  crates/oo-connectors/tests/parse_file_cli.rs
73cb1a070998c01edf6a87887678389f4ec6bad74cca07e120af165a02f657aa  contracts/connectors/parse-file-cli-v1.md
de5ad3f3c7bcf9368011aff422a4aa24043794231a49bde5d8679b0cf7acf0e0  crates/oo-connectors/src/bin/oo-file-parser-worker.rs
55760c6070b4449de5ae5fc70dca657ff550676e6f5614741dbe7552a33336cc  crates/oo-connectors/src/file_worker_parent.rs
e0c0466eb75c3de8acf4210bdc5a2ff5ccb9358a93ce19bb6abeca4964afbfd6  crates/oo-connectors/Cargo.toml
a277789df0b8c74febf113a2acc80a0a9f4b51bdeaa662e5d4f761be2489046b  Cargo.toml
16e6afda29ae4bae251d569f3c4ad16459527f0d0323598d8e4e0971818d458a  Cargo.lock
```

The review harness digest, only on the overlay archive, is `0564759838c6cc7dffa52c0b6eb90d3c131d62fbfd7a2067ca818cf643358365`.
