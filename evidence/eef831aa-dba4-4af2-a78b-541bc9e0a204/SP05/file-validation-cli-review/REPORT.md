# Report

Verdict: **APPROVE_LIMITED**.

UUID `eef831aa-dba4-4af2-a78b-541bc9e0a204`. Reviewed source `59be14baab1d5f30abaa2ce8cf67d607a2e4417f`, tree `d41c64a880ba4ef9f11775f5142bed1a2e7d76e9`, parent `ef6c4fc7da03342f900146e90ff692fe742d1ba4`. Author evidence `73d772c5cc7bac52c9e95754444f4c839a3ec646`. Date of this review: 2026-10-06.

The approval covers the `validate-file` command at that commit: one explicit CSV, TSV, or JSONL profile, a bounded request, and a success document only after `finish` returns a receipt. It does not close import, publication, PostgreSQL, HTTP, Arrow IPC, worker isolation, dialect sniffing, GB18030, Parquet, Excel, or a public authentication decision. The delimited reader approval is a199 on `671507b229ad7d1bc6a2e948b5b74d6d7252393f` (evidence `5b8cce62`). The JSONL reader approval is a185 on `47743688`. This review cites those approvals. It does not re-close them.

No product file, manifest, lock, or ledger was edited. The parent checklist was not changed. This directory is the review archive.

## What was read

- `evidence/eef831aa-dba4-4af2-a78b-541bc9e0a197/SP05/file-validation-cli/REPORT.md`, `FINDINGS.md`, and `HANDOFF.md`
- `contracts/pipeline/validate-file-cli-v1.md` at the fixed commit, saved as `source/blobs/validate-file-cli-v1.md`
- `source/blobs/validate-file.rs`, `validate_file_cli.rs`, `delimited.rs`, `jsonl.rs`, and `lib.rs`
- The a199 approval record, including `linux/overlay-3` and `tools/sha256_manifest.rs`
- `governance/review-guidance.md` for the rule that a slice approval does not mark the parent accepted

The executed tree is a `git archive` of `59be14ba`. Repo HEAD at the preoverlay launch was `84c03d01ce35728924100e42ca142c0ef650ac71`. Repo HEAD at the overlay-3 launch was `c27d6349ffa7cf8629a5a79c62015ca5c255e18f`. At the time of this report, `git diff 59be14ba HEAD` is empty for the three product files in that commit, and HEAD is `6247a03b508411bd89cfa06481122cb4868a6445`. The process that was tested is the sealed archive, not a later worktree.

## Pre-overlay smoke

`linux/preoverlay` ran before any review harness existed. The archive check refused `crates/oo-pipeline/tests/validate_file_a204_review.rs`. `blob-cmp.exit` 0. `overlay-absent.exit` 0. `hash-compare.exit` 0.

Rust `1.94.0` (`4a4ef493e 2026-03-02`), host `aarch64-unknown-linux-gnu`, cargo `1.94.0`, Debian 13, image `docker.io/library/rust@sha256:0f24958fa693fe838859faf64a0ec78d382cd543a6b6efd0fa7a3bcb10ea90d5`. `RUSTUP_TOOLCHAIN=1.94.0` overrides `rust-toolchain.toml` channel `1.98.1`. Container `f4edb34736e80c706c479c86e6043bf1b9245551eb939fc5ff57635ae27fbdcd`. Memory `8589934592`. `CapDrop=["ALL"]`. `OOMKilled=false`. `Privileged=false`. Network `oo-a204-eef831aa-net`.

`fetch-offline.exit` 101 is the cold private cargo volume (`chrono-tz` absent). The same script then ran `cargo fetch --locked` and recorded `fetch.exit` 0. `lib.exit` 0 (`11 passed`). `cli.exit` 0 is the one existing `validate_file_cli` pass (`11 passed`, 0.01s). `clippy-component.exit` 0. `clippy.exit` 0 on `--bin validate-file --test validate_file_cli -- -D warnings`. `docker-run.exit` 0. Two `sqlx-postgres` `unfulfilled_lint_expectations` warnings are in `clippy.stderr`. They are the locked vendor. Clippy still exited 0.

## Decisive integration run

`linux/overlay-3` copied `overlay/validate_file_a204_review.rs` onto a fresh archive of `59be14ba` as `crates/oo-pipeline/tests/validate_file_a204_review.rs`. The product blobs match `source/blobs`. `blob-cmp.exit` 0. `fetch.exit` 0 with `cargo fetch --locked --offline` on the warm private volume. `clippy-component.exit` 0. `test.exit` 0. `hash-compare.exit` 0. `clippy.exit` 0. `docker-run.exit` 0.

`test.stdout`: `ok. 10 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.31s`.

Container `483124ad01e5938972d03e928a550526218cdbed0a3664db199c656464f1d01e`. Same image, memory, cap drop, and privilege settings as the preoverlay run. `tmpfs /tmp` is `rw,nosuid,nodev,noexec,size=1g`. `/src` was read-only. The harness was not present in the preoverlay archive.

The harness spawns the compiled `validate-file` binary. It opens the same regular file through `DelimitedReader::open` or `JsonlReader::open`, folds lineage from the batches `next_batch` emitted, and calls `finish`. The CLI stdout is compared to that receipt. Profile digests are SHA-256 of a preimage built in the harness and checked against `profile_preimage` or `profile_digest`. The object hash is the harness SHA-256, which self-checks the empty digest and `abc`. `sha256sum` of the three saved fixtures equals those Rust digests (`linux/overlay-3/hash-compare.txt`). The harness does not link `sha2`. `OO_A197_ORACLE_DIR` is removed from the child environment. `OO_A204_ORACLE_DIR` receives the fixtures.

Clippy is `cargo clippy --locked --offline -p oo-pipeline --test validate_file_a204_review -- -D warnings`. Stderr ends `Finished dev profile [unoptimized + debuginfo] target(s) in 0.22s` after the same two vendor warnings. `clippy.stdout` is empty. The existing `validate_file_cli` suite was not run again in this container.

## Fresh receipts

Success stdout is one `oo.pipeline.validate-file-result/v1` object. The documents are `linux/overlay-3/oracle/positive-csv.stdout.json`, `positive-tsv.stdout.json`, and `positive-jsonl.stdout.json`. Each has `admits_import`, `published`, `arrow_ipc`, and `executes_native` false. The harness rejected cell text on stdout and stderr, including `熊猫`, `甲`, the decimal spellings, both `i64` extremes, `{"n":1}`, `SECRET_a204`, and `PATHTOKEN_a204`.

| Fixture | Rows | Batches | Value | Null | Empty | Missing | Object SHA-256 |
|---|---:|---:|---:|---:|---:|---:|---|
| CSV | 4 | 2 | 14 | 1 | 1 | 0 | `e316ece441b03a2ef125c5ac5810d3697c8269b54701aecd63d92ff701536445` |
| TSV | 3 | 2 | 11 | 1 | 0 | 0 | `5ef62398b756b7b6d2d10b9aa305ec49acf9ac92402c0abbfcd1e1f1fda9d66e` |
| JSONL | 4 | 2 | 13 | 1 | 0 | 2 | `62049328d535525064945cbf32c2a68ef6c2ac16c85989460e73d791097f8958` |
| Empty CSV | 0 | 0 | 0 | 0 | 0 | 0 | `e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855` |

CSV profile digest `bfba143084f4ff580adc246462a607aa808913ce4c9f2f680f40c8afc55db0ad`, config identity `e19ebc7c7196f6e4d5883224135929881768e5cedd12a19865095f6972a378d5`. First data line 2 at offset 21, last line 5 at offset 116. Batch sizes 2 and 2. Amount type in the summary is decimal precision 10 scale 2.

TSV profile digest `005d55a7dbfd184f6bc54344301b3a4df7e423a47e7d48bc6cbc36ec488f4bfa`, config identity `4b498ac5689b7c3a69434d89d3040776f7438163da54dca4211f2dd76eac5b98`. Lines 1 through 3, offsets 0 and 38. Batch sizes 2 and 1. Amount type is decimal precision 12 scale 2.

JSONL profile digest `93e264ea07e9bb58be0e9bb80f90275e7591752a34a972f94abb4ca438265957`, config identity `985ec0f9c4be96291e36e9ca486a555b258003a51c9a94f4621fbaf3adae6dd6`. Lines 1 through 4, offsets 0 and 170. Batch sizes 2 and 2. Payload type is `json_text`.

`ExactDecimal::parse_sql` then `rescale_exact(2)` on the fixture spellings: `1.20` coefficient `120`, `-0.00` coefficient `0` and non-negative, `1.2` coefficient `120`, `10` coefficient `1000`, `12.50` coefficient `1250`. `1.251` is `DecimalError::Inexact`. The command summary carries precision and scale. This run printed `arrow_decimal128=not_inspected`. `oo-pipeline` does not depend on `arrow-array`, and this review did not edit a manifest to add it. Arrow `Decimal128` coefficients for the public delimited reader are the a199 record on `671507b2`.

## Failures leave no success receipt

Observed on the compiled binary, with stdout empty and stderr one `oo.pipeline.validate-file-error/v1` object:

| Case | Exit | Outcome | Fact |
|---|---:|---|---|
| Empty file, header exact | 5 | parse | `header_required` |
| `1.251` at scale 2 | 5 | parse | `inexact_decimal` |
| `TRUE` | 5 | parse | `not_bool` |
| `01` | 5 | parse | `not_int64` |
| JSONL `1.0` | 5 | parse | `not_int64` |
| BOM with `bom=refuse` | 5 | parse | `bom_refused` |
| `max_rows` 1 | 6 | budget | `max_rows` |
| Flipped object digest | 7 | hash | `hash_mismatch` |
| `cancel_before_read` | 8 | cancel | `cancelled` |
| Missing path, cancel set in the request | 4 | read | `open` |
| Directory | 4 | read | `not_file` |
| Duplicate `protocol` key | 3 | config | `request_json` |
| Extra `password` | 3 | config | `unknown_field` |
| Uppercase `expected_sha256` | 3 | config | `expected_sha256` |
| `format=parquet` | 3 | config | `format` |
| CSV dialect `tsv` | 3 | config | `dialect` |
| Null token comma | 3 | config | `null_token` |
| Column type `float` | 3 | config | `type` |
| JSONL `dialect` or `max_field_bytes` | 3 | config | `unknown_field` |
| JSONL type `decimal` | 3 | config | `type` |
| Deadline 0 and 120001 | 3 | config | `deadline` |
| JSON null `protocol` | 3 | config | `request_field` |
| 1048577 request bytes | 3 | config | `request_bytes` |
| 1048576 `{` bytes | 3 | config | `request_json` |
| No args, or `--request` without a path | 2 | usage | `usage` |
| `--nope`, `-h`, `--help` plus an argument, `--request` plus `--extra` | 2 | usage | `unknown_flag` |

`--help` exits 0, writes the usage text to stdout, and leaves stderr empty. The text contains the request protocol, `1048576`, CSV, TSV, JSONL, and the statements that the command does not infer a dialect, does not import, does not publish a table, does not open a database, does not write Arrow IPC, does not interrupt a blocking read, and does not authenticate a public caller.

A symlink to the CSV is followed. The finish receipt has 4 rows. Stdout contains the result protocol and does not contain the link path. That is evidence that `File::open` follows links. It is not a path jail and it is not an authorization decision. A missing file is `open` even when `cancel_before_read` is true, because the command opens the path before it sets the cancel flag.

## Preserved attempts

- `linux/overlay`: the harness called `expect_err` on `Result<DelimitedSeen, _>` and `Result<JsonlSeen, _>` before those structs implemented `Debug`. `test.exit` 101, `clippy.exit` `not_run`, `docker-run.exit` 101. Container `627672b8c14dbba61b49a245fe219f2caa0991b7c275a1d31429c4d332341ae7`. Nine `E0277` errors, first at the harness line that is `expect_err("header")`.
- `linux/overlay-2`: the same 10 tests passed (`0.32s`), `hash-compare.exit` 0, then clippy `-D warnings` rejected unread `admits_import`, `published`, `arrow_ipc`, and `executes_native` fields. `clippy.exit` 101, `docker-run.exit` 101. Container `9c6e1b6f224dd8618bb8693888eb4a1661e4a271d39004d97b19ad8855a6e8b1`.

Those directories were not overwritten. The product sources in them match the fixed commit. The harness change after overlay-2 is an assertion of the closed flags and the profile id. It is in `overlay/validate_file_a204_review.rs` and in `linux/overlay-3/expect-review.rs`.

## Limits

The input is a regular file on a trusted local path. Cancel and the deadline are checked before each blocking read. This review did not time a read that stays blocked for the whole call. a199 recorded that behavior for the delimited reader (80 ms still blocked, 2009 ms on an 800 ms deadline). The command help text states the same limit.

`queue_batches` stays 1 inside the readers. The harness set `batch_rows` to 2 so the batch count is not the row count. Live PostgreSQL tests were not started. This task used no PostgreSQL container.

## Sealed blob digests

`sha256sum` inside the overlay-3 container:

```text
a562c5a90e03bb98d47ab6c765be57bdac93e06cf70bdb47b84ed6e23d92aea2  crates/oo-pipeline/src/bin/validate-file.rs
1cbce75887f446eb3300b98e076a344921e627bd842cc4f91d6a96d2db4b4f19  crates/oo-pipeline/tests/validate_file_cli.rs
2e17088adc741e2414ef48cb6987beb2530e07d46fb401ec715cf442718340bf  contracts/pipeline/validate-file-cli-v1.md
12583452df440852b8979d0dfa966966eebbbc53a04433d1c7bc41aa60cd4587  crates/oo-connectors/src/delimited.rs
55056174f5ac1216c23590471b6ed40e84f2e84de940626bc27f564aeae99764  crates/oo-connectors/src/jsonl.rs
5ca17a403e2e8ee01bf074bc81af8a261ffd639a2aa660610cf0c240aaf0007d  crates/oo-connectors/src/lib.rs
```

The same six digests are in `linux/preoverlay/archive-sha256.txt`. The overlay harness digest is only in `linux/overlay-3/source-sha256.txt` (`5b54bc683bedc01a9be2439d09ea7c8b581d1576ffed99b9bdcb3750ffa0821f`). `MANIFEST.sha256` is the std-only Rust manifest of this directory.
